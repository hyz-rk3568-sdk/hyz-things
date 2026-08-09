pub mod domain;

#[cfg(all(feature = "native", not(target_os = "linux")))]
compile_error!("the native router implementation requires Linux");

#[cfg(all(feature = "native", target_os = "linux"))]
pub mod adapters;
#[cfg(all(feature = "native", target_os = "linux"))]
pub mod application;
