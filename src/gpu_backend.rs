//! Which wgpu backends to draw with: `backend` in config.toml, else `auto`.
//!
//! Windows draws with GL first when the machine has it: under Vulkan and DX12
//! an AMD driver thread spins a core for a window that is doing nothing
//! (filer #232, #240), which a system monitor would then report about
//! itself. Elsewhere wgpu chooses. The same rules as tsumugi-pane's `gpu.rs`,
//! copied rather than depending on the whole pane for them.

use eframe::wgpu::{self, Backends};

/// The backends to narrow wgpu to for a backend the user named (`None` for
/// `auto`): `Ok` with what to use (`None` leaves wgpu its own pick), or `Err`
/// with `auto`'s choice when this machine has no adapter for the named one.
fn pick_backends(name: Option<&str>, windows: bool, has: impl Fn(Backends) -> bool) -> Result<Option<Backends>, Option<Backends>> {
    let auto = || auto_backends(windows, || has(Backends::GL));
    let Some(name) = name else { return Ok(auto()) };
    let backends = Backends::from_comma_list(name);
    match has(backends) {
        true => Ok(Some(backends)),
        false => Err(auto()),
    }
}

/// What `auto` narrows the backends to: GL on Windows when this machine has
/// it, else nothing. `gl_ok` is only asked on Windows, since the question
/// costs an instance of its own.
fn auto_backends(windows: bool, gl_ok: impl FnOnce() -> bool) -> Option<Backends> {
    (windows && gl_ok()).then_some(Backends::GL)
}

/// Whether wgpu finds an adapter on `backends`, asked of an instance of its
/// own before the window exists; a probe still pending is taken as a yes.
fn has_adapter(backends: Backends) -> bool {
    use std::future::Future as _;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let mut probe = std::pin::pin!(instance.enumerate_adapters(backends));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match probe.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(adapters) => !adapters.is_empty(),
        std::task::Poll::Pending => true,
    }
}

/// The wgpu setup for `backend`, and a warning when it could not be had.
/// `WGPU_BACKEND` wins: it is the one-run override.
pub fn options(backend: Option<&str>) -> (eframe::WgpuConfiguration, Option<String>) {
    use eframe::egui_wgpu::WgpuSetup;
    let mut options = eframe::WgpuConfiguration::default();
    if std::env::var_os("WGPU_BACKEND").is_some_and(|v| !v.is_empty()) {
        return (options, None);
    }
    let (backends, warning) = match pick_backends(backend, cfg!(windows), has_adapter) {
        Ok(b) => (b, None),
        Err(auto) => (
            auto,
            Some(format!(
                "config.toml: backend = \"{}\": this machine has no adapter for it; drawing with the default",
                backend.unwrap_or_default()
            )),
        ),
    };
    if let (Some(backends), WgpuSetup::CreateNew(setup)) = (backends, &mut options.wgpu_setup) {
        setup.instance_descriptor.backends = backends;
    }
    (options, warning)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_backend_falls_back_to_auto() {
        let gl_only = |b: Backends| b == Backends::GL;
        assert_eq!(pick_backends(Some("vulkan"), true, gl_only), Err(Some(Backends::GL)));
        assert_eq!(pick_backends(Some("vulkan"), true, |_| true), Ok(Some(Backends::VULKAN)));
        assert_eq!(pick_backends(None, true, gl_only), Ok(Some(Backends::GL)));
        assert_eq!(pick_backends(None, false, |_| true), Ok(None), "auto off Windows: wgpu's pick");
    }

    #[test]
    fn auto_is_gl_on_windows_only() {
        assert_eq!(auto_backends(true, || true), Some(Backends::GL));
        assert_eq!(auto_backends(true, || false), None);
        assert_eq!(auto_backends(false, || panic!("asked about GL off Windows")), None);
    }
}
