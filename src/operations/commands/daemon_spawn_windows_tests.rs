use super::*;

fn flags(handle: HANDLE) -> u32 {
    let mut flags = 0;
    assert_ne!(unsafe { GetHandleInformation(handle, &mut flags) }, 0);
    flags
}

fn set_inherit(handle: HANDLE, value: u32) {
    assert_ne!(
        unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, value) },
        0
    );
}

#[test]
fn aliased_standard_handles_restore_the_original_inheritance_flag() {
    let file = tempfile::tempfile().unwrap();
    let handle = file.as_raw_handle();
    let original = flags(handle);
    set_inherit(handle, HANDLE_FLAG_INHERIT);
    {
        let _guard = StdioInheritanceGuard::new([handle, handle, std::ptr::null_mut()]).unwrap();
        assert_eq!(flags(handle) & HANDLE_FLAG_INHERIT, 0);
    }
    assert_eq!(flags(handle) & HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    set_inherit(handle, original);
}

#[test]
fn missing_and_noninheritable_standard_handles_are_unchanged() {
    let file = tempfile::tempfile().unwrap();
    let handle = file.as_raw_handle();
    let original = flags(handle);
    assert_eq!(original & HANDLE_FLAG_INHERIT, 0);
    {
        let _guard =
            StdioInheritanceGuard::new([handle, std::ptr::null_mut(), INVALID_HANDLE_VALUE])
                .unwrap();
        assert_eq!(flags(handle), original);
    }
    assert_eq!(flags(handle), original);
}

#[test]
fn failed_spawn_restores_handle_inheritance_on_early_return() {
    let file = tempfile::tempfile().unwrap();
    let handle = file.as_raw_handle();
    let original = flags(handle);
    set_inherit(handle, HANDLE_FLAG_INHERIT);
    let dir = tempfile::tempdir().unwrap();
    let result = (|| -> std::io::Result<()> {
        let _guard =
            StdioInheritanceGuard::new([handle, std::ptr::null_mut(), INVALID_HANDLE_VALUE])?;
        Command::new(dir.path().join("missing-executable.exe")).spawn()?;
        Ok(())
    })();
    assert!(result.is_err());
    assert_eq!(flags(handle) & HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT);
    set_inherit(handle, original);
}
