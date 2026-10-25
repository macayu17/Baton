use super::*;
use windows_permissions::{
    constants::{AceFlags, AceType, SeObjectType, SecurityInformation},
    utilities, wrappers, LocalBox, SecurityDescriptor,
};

pub(super) fn principal() -> Result<String, Diagnostic> {
    let sid = utilities::current_process_sid().map_err(|_| failure(ErrorCode::Forbidden))?;
    let text = wrappers::ConvertSidToStringSid(&sid).map_err(|_| failure(ErrorCode::Forbidden))?;
    Ok(format!(
        "windows:{}",
        text.to_str().ok_or_else(|| failure(ErrorCode::Forbidden))?
    ))
}

pub(super) fn validate(path: &Path, private: bool) -> Result<(), Diagnostic> {
    let sid = utilities::current_process_sid().map_err(|_| failure(ErrorCode::Forbidden))?;
    let sd = wrappers::GetNamedSecurityInfo(
        path.as_os_str(),
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Owner | SecurityInformation::Dacl,
    )
    .map_err(|_| failure(ErrorCode::Forbidden))?;
    if sd.owner() != Some(&*sid) {
        return Err(failure(ErrorCode::Forbidden));
    }
    let acl = sd.dacl().ok_or_else(|| failure(ErrorCode::Forbidden))?;
    let system: LocalBox<windows_permissions::Sid> = "S-1-5-18"
        .parse()
        .map_err(|_| failure(ErrorCode::Forbidden))?;
    let admins: LocalBox<windows_permissions::Sid> = "S-1-5-32-544"
        .parse()
        .map_err(|_| failure(ErrorCode::Forbidden))?;
    let mut owner_allowed = false;
    for index in 0..acl.len() {
        let ace = acl
            .get_ace(index)
            .ok_or_else(|| failure(ErrorCode::Forbidden))?;
        // Refuse callback/object/unknown ACE semantics instead of guessing access.
        if ace.ace_type() != AceType::ACCESS_ALLOWED_ACE_TYPE {
            return Err(failure(ErrorCode::Forbidden));
        }
        let subject = ace.sid().ok_or_else(|| failure(ErrorCode::Forbidden))?;
        if subject == &*sid {
            owner_allowed |= !ace.flags().contains(AceFlags::InheritOnly);
        } else if subject != &*system && subject != &*admins {
            let write = 0x1000_0000 | 0x4000_0000 | 0x000D_0156;
            if private
                || (!ace.flags().contains(AceFlags::InheritOnly) && ace.mask().bits() & write != 0)
            {
                return Err(failure(ErrorCode::Forbidden));
            }
        }
    }
    if !owner_allowed {
        return Err(failure(ErrorCode::Forbidden));
    }
    Ok(())
}

pub(super) fn protect_new(path: &Path) -> Result<(), Diagnostic> {
    let subject = principal()?;
    let sid = subject
        .strip_prefix("windows:")
        .ok_or_else(|| failure(ErrorCode::Forbidden))?;
    let sd: LocalBox<SecurityDescriptor> = format!("D:P(A;OICI;FA;;;{sid})")
        .parse()
        .map_err(|_| failure(ErrorCode::Forbidden))?;
    wrappers::SetNamedSecurityInfo(
        path.as_os_str(),
        SeObjectType::SE_FILE_OBJECT,
        SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
        None,
        None,
        sd.dacl(),
        None,
    )
    .map_err(|_| failure(ErrorCode::Forbidden))
}
