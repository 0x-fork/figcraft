//! Which graphics API draws the window.
//!
//! eframe offers wgpu every backend it was built with, and between equal GPUs wgpu takes Vulkan
//! first, on Windows too. A Vulkan driver that faults there takes the app down as the window opens:
//! an access violation inside the vendor's driver, which no guard of ours can catch (seen on a laptop
//! with two AMD GPUs whose drivers were years apart). Direct3D 12 is part of Windows, so on Windows
//! wgpu is offered Direct3D 12, and OpenGL for hardware without a Direct3D 12 driver.
//! Windows Direct3D 12 uses DirectComposition by default so window resizing keeps the UI sharp.
//! `WGPU_DX12_PRESENTATION_SYSTEM` can select `Visual` or roll back to `Hwnd`.
//!
//! `WGPU_BACKEND` (`dx12`, `vulkan`, `gl`, `metal`) still names the backend on every system, and
//! systems other than Windows keep eframe's defaults.

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{Backends, Dx12SwapchainKind};

/// Direct3D 12, then OpenGL: wgpu ranks a hardware GPU on either ahead of Direct3D's software one.
const WINDOWS: Backends = Backends::DX12.union(Backends::GL);

/// The backends wgpu chooses from: the ones `WGPU_BACKEND` asks for (`requested`), else [`WINDOWS`]
/// on Windows, else eframe's `defaults`.
pub fn backends(requested: Option<Backends>, windows: bool, defaults: Backends) -> Backends {
    match requested {
        Some(requested) => requested,
        None if windows => WINDOWS,
        None => defaults,
    }
}

/// The DX12 presentation system: explicit setting wins, else Windows uses DirectComposition.
pub(super) fn presentation_system(requested: Option<Dx12SwapchainKind>, windows: bool, defaults: Dx12SwapchainKind) -> Dx12SwapchainKind {
    match requested {
        Some(requested) => requested,
        None if windows => Dx12SwapchainKind::DxgiFromVisual,
        None => defaults,
    }
}

/// Offer wgpu [`backends`] instead of eframe's defaults.
pub fn configure(options: &mut WgpuConfiguration) {
    configure_for_platform(options, Backends::from_env(), Dx12SwapchainKind::from_env(), cfg!(windows));
}

pub(super) fn configure_for_platform(
    options: &mut WgpuConfiguration,
    requested_backends: Option<Backends>,
    requested_presentation: Option<Dx12SwapchainKind>,
    windows: bool,
) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_setup {
        let offered = &mut create.instance_descriptor.backends;
        *offered = backends(requested_backends, windows, *offered);
        if windows {
            let presentation = &mut create.instance_descriptor.backend_options.dx12.presentation_system;
            *presentation = presentation_system(requested_presentation, windows, *presentation);
        }
    }
}
