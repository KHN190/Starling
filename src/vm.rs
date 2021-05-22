const WREN_VERSION_NUMBER: &'static str = env!("CARGO_PKG_VERSION");

pub fn wren_get_version_number() -> &'static str
{
  return WREN_VERSION_NUMBER;
}

pub struct WrenVM;
