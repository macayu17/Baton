pub use baton_platform::process::{LaunchSpec, TreeStatus};
use baton_protocol::*;
use baton_store::{Actor, LaunchPermit, Store, ToolPermit};
use std::{
    thread,
    time::{Duration, Instant},
};
mod native;

fn error(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}

/// Live in-memory control; neither a PID nor a persisted RunRecord recreates it.
pub struct Session {
    binding: RunBinding,
    transfer: Option<Id>,
    native: native::Native,
    workspace: baton_platform::workspace::Workspace,
}
impl Session {
    pub fn launch(
        store: &mut Store,
        actor: &Actor,
        permit: LaunchPermit,
        spec: LaunchSpec,
    ) -> Result<Self, Diagnostic> {
        store.validate_launch(actor, &permit)?;
        if spec.fingerprint() != permit.record().launch_fingerprint {
            return Err(error(ErrorCode::InvalidInput));
        }
        let binding = permit.record().binding.clone();
        let transfer = permit.transfer_id();
        let workspace = baton_platform::workspace::Workspace::open(spec.working_directory())?;
        let native = match native::Native::spawn(spec) {
            Ok(child) => child,
            Err(e) => {
                let _ = record(
                    store,
                    actor,
                    &binding,
                    Id::new_v4(),
                    RunObservation::SupervisorLost,
                );
                return Err(e);
            }
        };
        let session = Self {
            binding,
            transfer,
            native,
            workspace,
        };
        // Failure drops the live control, terminating the managed tree.
        store.record_native_spawn(actor, &permit, &session.native, Id::new_v4())?;
        Ok(session)
    }
    pub fn binding(&self) -> &RunBinding {
        &self.binding
    }
    pub fn working_directory(&self) -> &std::path::Path {
        self.native.working_directory()
    }
    pub fn take_stdio(
        &mut self,
    ) -> Result<(std::process::ChildStdin, std::process::ChildStdout), Diagnostic> {
        self.native.take_stdio()
    }
    /// Trusted host transport loss; observed history cannot reconstruct this live control.
    pub fn connection_lost(
        &mut self,
        store: &mut Store,
        actor: &Actor,
    ) -> Result<RunRecord, Diagnostic> {
        let stopped = self.native.terminate();
        let recorded = record(
            store,
            actor,
            &self.binding,
            Id::new_v4(),
            RunObservation::SupervisorLost,
        );
        stopped?;
        recorded
    }
    pub fn prepare_tool(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        request: &Request,
    ) -> Result<(RunRecord, Option<ToolPermit>), Diagnostic> {
        let Action::ObserveRun {
            task_id,
            run_id,
            lease_id,
            ..
        } = request.action
        else {
            return Err(error(ErrorCode::InvalidInput));
        };
        if task_id != self.binding.task_id
            || run_id != self.binding.run_id
            || lease_id != self.binding.lease_id
            || request.expected_epoch != self.binding.epoch
        {
            return Err(error(ErrorCode::Forbidden));
        }
        let proof = self.native.running_proof()?;
        store.prepare_tool(actor, request, &proof)
    }
    pub fn authorize_tool(
        &mut self,
        store: &Store,
        actor: &Actor,
        permit: ToolPermit,
    ) -> Result<(), Diagnostic> {
        if permit.record().binding != self.binding {
            return Err(error(ErrorCode::Forbidden));
        }
        let proof = self.native.running_proof()?;
        store.consume_tool(actor, permit, &proof)
    }
    /// Trusted host create-only operation. Historical intent never replays a write.
    pub fn create_file(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        operation_id: Id,
        name: &str,
        bytes: &[u8],
    ) -> Result<String, Diagnostic> {
        baton_platform::workspace::Workspace::validate_name(name)?;
        if bytes.len() > 16_384 || operation_id.is_nil() {
            return Err(error(ErrorCode::InvalidInput));
        }
        let prior = store.execution_event(
            actor,
            self.binding.task_id,
            self.binding.run_id,
            operation_id,
        )?;
        let sequence = match prior.map(|event| event.observation) {
            Some(RunObservation::ToolReserved {
                source_sequence, ..
            }) => source_sequence,
            Some(_) => return Err(error(ErrorCode::IdempotencyConflict)),
            None => store
                .run(actor, self.binding.task_id, self.binding.run_id)?
                .source_cursor
                .checked_add(1)
                .ok_or_else(|| error(ErrorCode::Blocked))?,
        };
        let mut hash = blake3::Hasher::new();
        hash.update(b"baton-host-create-file-v1");
        for id in [
            self.binding.task_id,
            self.binding.run_id,
            self.binding.actor_id,
            self.binding.lease_id,
            operation_id,
        ] {
            hash.update(id.as_bytes());
        }
        hash.update(&self.binding.epoch.to_le_bytes());
        hash.update(&sequence.to_le_bytes());
        hash.update(&(name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update(bytes);
        let mut identity = [0; 16];
        identity.copy_from_slice(&hash.finalize().as_bytes()[..16]);
        let effect_id = Id::from_bytes(identity);
        let request = Request {
            schema_version: SchemaVersion::V0_1_0,
            operation_id,
            actor_id: actor.id(),
            expected_epoch: self.binding.epoch,
            action: Action::ObserveRun {
                task_id: self.binding.task_id,
                run_id: self.binding.run_id,
                lease_id: self.binding.lease_id,
                observation: RunObservation::ToolReserved {
                    source_sequence: sequence,
                    effect_id,
                    class: EffectClass::LocalReversible,
                },
            },
        };
        let (_, permit) = self.prepare_tool(store, actor, &request)?;
        let permit = permit.ok_or_else(|| error(ErrorCode::Blocked))?;
        let result = (|| {
            self.workspace.verify()?;
            self.authorize_tool(store, actor, permit)?;
            record(
                store,
                actor,
                &self.binding,
                Id::new_v4(),
                RunObservation::ToolStarted {
                    source_sequence: sequence
                        .checked_add(1)
                        .ok_or_else(|| error(ErrorCode::Blocked))?,
                    effect_id,
                    class: EffectClass::LocalReversible,
                },
            )?;
            let digest = self.workspace.create_file(name, bytes)?;
            record(
                store,
                actor,
                &self.binding,
                Id::new_v4(),
                RunObservation::ToolFinished {
                    source_sequence: sequence
                        .checked_add(2)
                        .ok_or_else(|| error(ErrorCode::Blocked))?,
                    effect_id,
                    outcome: ToolOutcome::Succeeded,
                },
            )?;
            Ok(digest)
        })();
        if result.is_err() {
            let _ = self.connection_lost(store, actor);
        }
        result
    }
    /// Normalize/validate the adapter acknowledgement before calling this trusted host boundary.
    pub fn acknowledge_receiver(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        operation_id: Id,
        hello: &ReceiverSession,
    ) -> Result<TransferRecord, Diagnostic> {
        let transfer = self.transfer.ok_or_else(|| error(ErrorCode::Forbidden))?;
        let result = (|| {
            if hello.binding != self.binding {
                return Err(error(ErrorCode::Forbidden));
            }
            let proof = self.native.running_proof()?;
            store.record_receiver_ready(actor, transfer, hello, &proof, operation_id)
        })();
        if result.is_err() {
            let _ = self.native.terminate();
            let _ = record(
                store,
                actor,
                &self.binding,
                Id::new_v4(),
                RunObservation::SupervisorLost,
            );
        }
        result
    }
    pub fn root_exited(&mut self) -> Result<bool, Diagnostic> {
        self.native.root_exited()
    }
    pub fn tree_status(&mut self) -> Result<TreeStatus, Diagnostic> {
        self.native.status()
    }

    pub fn request_interrupt(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        interrupt: impl FnOnce() -> Result<(), Diagnostic>,
    ) -> Result<(), Diagnostic> {
        record(
            store,
            actor,
            &self.binding,
            Id::new_v4(),
            RunObservation::InterruptRequested,
        )?;
        interrupt()
    }
    /// Adapter notifications cannot fabricate supervisor process facts.
    pub fn observe(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        operation_id: Id,
        observation: RunObservation,
    ) -> Result<RunRecord, Diagnostic> {
        if !matches!(
            observation,
            RunObservation::ToolStarted { .. }
                | RunObservation::StartupConfigReported { .. }
                | RunObservation::ThreadRegistered { .. }
                | RunObservation::ThreadAnnounced { .. }
                | RunObservation::RemoteControlDisabled { .. }
                | RunObservation::ToolFinished { .. }
                | RunObservation::InterruptAcknowledged { .. }
                | RunObservation::AdapterReported { .. }
                | RunObservation::AdapterGap { .. }
        ) {
            return Err(error(ErrorCode::Forbidden));
        }
        let result = record(store, actor, &self.binding, operation_id, observation);
        if result.is_err()
            || store.task(actor, self.binding.task_id).is_ok_and(|task| {
                matches!(task.state, TaskState::Suspect | TaskState::RecoveryRequired)
            })
        {
            let _ = self.native.terminate();
            let _ = record(
                store,
                actor,
                &self.binding,
                Id::new_v4(),
                RunObservation::SupervisorLost,
            );
        }
        result
    }
    /// Stop own children even when authority/journal access is lost. Never replay effects.
    pub fn stop(
        &mut self,
        store: &mut Store,
        actor: &Actor,
        timeout: Duration,
    ) -> Result<RunRecord, Diagnostic> {
        if timeout.is_zero() || timeout > Duration::from_secs(30) {
            return Err(error(ErrorCode::InvalidInput));
        }
        self.native.terminate()?;
        let until = Instant::now() + timeout;
        loop {
            match self.native.status()? {
                TreeStatus::Empty => {
                    let current = store.run(actor, self.binding.task_id, self.binding.run_id)?;
                    if current.state == RunState::Stopped {
                        return Ok(current);
                    }
                    let proof = self.native.drain_proof()?;
                    let result =
                        store.record_native_drain(actor, &self.binding, &proof, Id::new_v4());
                    if result.is_err() {
                        let _ = record(
                            store,
                            actor,
                            &self.binding,
                            Id::new_v4(),
                            RunObservation::SupervisorLost,
                        );
                    }
                    return result;
                }
                TreeStatus::Unknown if self.native.root_exited()? => {
                    return record(
                        store,
                        actor,
                        &self.binding,
                        Id::new_v4(),
                        RunObservation::SupervisorLost,
                    );
                }
                _ => (),
            }
            if Instant::now() >= until {
                let _ = record(
                    store,
                    actor,
                    &self.binding,
                    Id::new_v4(),
                    RunObservation::SupervisorLost,
                );
                return Err(error(ErrorCode::DrainTimeout));
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn record(
    store: &mut Store,
    actor: &Actor,
    binding: &RunBinding,
    operation_id: Id,
    observation: RunObservation,
) -> Result<RunRecord, Diagnostic> {
    let request = Request {
        schema_version: SchemaVersion::V0_1_0,
        operation_id,
        actor_id: actor.id(),
        expected_epoch: binding.epoch,
        action: Action::ObserveRun {
            task_id: binding.task_id,
            lease_id: binding.lease_id,
            run_id: binding.run_id,
            observation,
        },
    };
    match store.execute(actor, &request)? {
        Reply::Run(record) => Ok(record),
        _ => Err(error(ErrorCode::CorruptStore)),
    }
}
