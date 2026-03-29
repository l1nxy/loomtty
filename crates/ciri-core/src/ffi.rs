//! C ABI for embedding ciri-core in native platform shells.
//! Currently a stub — will be implemented when the macOS Swift shell is built.

// Future API:
// #[no_mangle] pub extern "C" fn ciri_core_new(...) -> *mut CoreApp
// #[no_mangle] pub extern "C" fn ciri_core_destroy(app: *mut CoreApp)
// #[no_mangle] pub extern "C" fn ciri_core_handle_key(...)
// #[no_mangle] pub extern "C" fn ciri_core_tick(...)
