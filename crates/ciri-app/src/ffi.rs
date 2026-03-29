//! C ABI for embedding ciri-app in native platform shells.
//! Currently a stub — will be implemented when the macOS Swift shell is built.

// Future API:
// #[no_mangle] pub extern "C" fn ciri_app_new(...) -> *mut AppModel
// #[no_mangle] pub extern "C" fn ciri_app_destroy(app: *mut AppModel)
// #[no_mangle] pub extern "C" fn ciri_app_handle_key(...)
// #[no_mangle] pub extern "C" fn ciri_app_tick(...)
