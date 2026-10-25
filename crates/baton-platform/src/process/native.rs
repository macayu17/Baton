use super::*;
use process_wrap::std::{ChildWrapper, CommandWrap};
#[cfg(windows)]
use std::sync::Arc;

pub(super) struct Native {
    child: Box<dyn ChildWrapper>,
    root_exited: bool,
    #[cfg(windows)]
    job: Arc<win32job::Job>,
    #[cfg(unix)]
    killed: bool,
}

#[cfg(windows)]
#[derive(Debug)]
struct AssignBeforeResume(Arc<win32job::Job>);
#[cfg(windows)]
impl process_wrap::std::CommandWrapper for AssignBeforeResume {
    fn post_spawn(
        &mut self,
        _command: &mut Command,
        child: &mut std::process::Child,
        _core: &CommandWrap,
    ) -> std::io::Result<()> {
        use std::os::windows::io::AsRawHandle;
        if self
            .0
            .assign_process(child.as_raw_handle() as isize)
            .is_err()
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::other("job assignment failed"));
        }
        Ok(())
    }
}

impl Native {
    pub(super) fn spawn(command: Command) -> Result<Self, Diagnostic> {
        let mut wrap = CommandWrap::from(command);
        #[cfg(windows)]
        let job = {
            use process_wrap::std::{CreationFlags, JobObject};
            use windows::Win32::System::Threading::CREATE_NO_WINDOW;
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            let job = Arc::new(
                win32job::Job::create_with_limit_info(&limits)
                    .map_err(|_| error(ErrorCode::SupervisionFailure))?,
            );
            // process-wrap suspends creation; post_spawn assigns our queried job
            // before its JobObject wrapper resumes the child. No breakaway flags.
            wrap.wrap(AssignBeforeResume(job.clone()))
                .wrap(CreationFlags(CREATE_NO_WINDOW))
                .wrap(JobObject);
            job
        };
        #[cfg(unix)]
        wrap.wrap(process_wrap::std::ProcessGroup::leader());
        #[cfg(not(any(windows, unix)))]
        return Err(error(ErrorCode::SupervisionFailure));
        let child = wrap
            .spawn()
            .map_err(|_| error(ErrorCode::SupervisionFailure))?;
        Ok(Self {
            child,
            root_exited: false,
            #[cfg(windows)]
            job,
            #[cfg(unix)]
            killed: false,
        })
    }
    pub(super) fn pid(&self) -> u32 {
        self.child.id()
    }
    pub(super) fn take_stdio(&mut self) -> Result<(ChildStdin, ChildStdout), Diagnostic> {
        match (self.child.stdin().take(), self.child.stdout().take()) {
            (Some(stdin), Some(stdout)) => Ok((stdin, stdout)),
            _ => Err(error(ErrorCode::SupervisionFailure)),
        }
    }
    pub(super) fn root_exited(&mut self) -> Result<bool, Diagnostic> {
        if !self.root_exited {
            self.root_exited = self
                .child
                .try_wait()
                .map_err(|_| error(ErrorCode::SupervisionFailure))?
                .is_some();
        }
        Ok(self.root_exited)
    }
    pub(super) fn status(&mut self) -> Result<TreeStatus, Diagnostic> {
        #[cfg(windows)]
        {
            let processes = self
                .job
                .query_process_id_list()
                .map_err(|_| error(ErrorCode::SupervisionFailure))?;
            if processes.is_empty() {
                self.root_exited()?;
                Ok(TreeStatus::Empty)
            } else {
                Ok(TreeStatus::Active)
            }
        }
        #[cfg(unix)]
        {
            // shortcut: process groups cannot prove escaped descendants absent;
            // require a containment backend before enabling verified Unix drain.
            if self.root_exited()? {
                Ok(TreeStatus::Unknown)
            } else {
                Ok(TreeStatus::Active)
            }
        }
        #[cfg(not(any(windows, unix)))]
        Ok(TreeStatus::Unknown)
    }
    pub(super) fn terminate(&mut self) -> Result<(), Diagnostic> {
        #[cfg(windows)]
        {
            self.child
                .start_kill()
                .map_err(|_| error(ErrorCode::SupervisionFailure))
        }
        #[cfg(unix)]
        {
            // Never signal a potentially recycled group ID after reaping its leader.
            if !self.root_exited && !self.killed {
                self.child
                    .start_kill()
                    .map_err(|_| error(ErrorCode::SupervisionFailure))?;
                self.killed = true;
            }
            Ok(())
        }
        #[cfg(not(any(windows, unix)))]
        Err(error(ErrorCode::SupervisionFailure))
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.terminate();
        #[cfg(unix)]
        if !self.root_exited && self.killed {
            let _ = self.child.wait();
        }
        // Windows closes the non-inherited kill-on-close handle on ordinary drop
        // and OS teardown. Unix crash containment remains explicitly unsupported.
    }
}
