//! The backends offered to wgpu: no Vulkan on Windows unless it is asked for (a faulting Vulkan
//! driver crashed the app as its window opened), and nothing changed anywhere else.

use eframe::egui_wgpu::{WgpuConfiguration, WgpuSetup};
use eframe::wgpu::{Backends, Dx12SwapchainKind};

use crate::graphics::{backends, configure_for_platform, presentation_system};

/// What eframe offers when `WGPU_BACKEND` isn't set.
const DEFAULTS: Backends = Backends::PRIMARY.union(Backends::GL);

#[test]
fn windows_is_offered_direct3d_and_not_vulkan() {
    let offered = backends(None, true, DEFAULTS);
    assert!(offered.contains(Backends::DX12), "Direct3D 12 draws the window");
    assert!(offered.contains(Backends::GL), "OpenGL is there for hardware without a Direct3D 12 driver");
    assert!(!offered.contains(Backends::VULKAN), "a Vulkan driver that faults can't be picked");
}

#[test]
fn wgpu_backend_names_the_backend_on_every_system() {
    for windows in [true, false] {
        for asked in [Backends::VULKAN, Backends::DX12, Backends::GL, Backends::VULKAN | Backends::GL] {
            assert_eq!(backends(Some(asked), windows, DEFAULTS), asked, "windows: {windows}");
        }
    }
}

#[test]
fn other_systems_keep_eframes_defaults() {
    assert_eq!(backends(None, false, DEFAULTS), DEFAULTS);
    assert_eq!(backends(None, false, Backends::METAL), Backends::METAL);
}

#[test]
fn windows_defaults_to_direct_composition_visual() {
    assert_eq!(presentation_system(None, true, Dx12SwapchainKind::DxgiFromHwnd), Dx12SwapchainKind::DxgiFromVisual);
}

#[test]
fn valid_presentation_override_wins_on_all_platforms() {
    for requested in [Dx12SwapchainKind::DxgiFromHwnd, Dx12SwapchainKind::DxgiFromVisual] {
        for windows in [true, false] {
            assert_eq!(presentation_system(Some(requested), windows, Dx12SwapchainKind::DxgiFromVisual), requested);
        }
    }
}

#[test]
fn other_platforms_keep_existing_presentation_system() {
    assert_eq!(presentation_system(None, false, Dx12SwapchainKind::DxgiFromHwnd), Dx12SwapchainKind::DxgiFromHwnd);
}

#[test]
fn create_new_configuration_uses_windows_default_and_explicit_override() {
    let mut options = WgpuConfiguration::default();
    set_presentation_system(&mut options, Dx12SwapchainKind::DxgiFromHwnd);
    configure_for_platform(&mut options, None, None, true);
    assert_eq!(configured_presentation_system(&options), Some(Dx12SwapchainKind::DxgiFromVisual));

    for requested in [Dx12SwapchainKind::DxgiFromHwnd, Dx12SwapchainKind::DxgiFromVisual] {
        let mut options = WgpuConfiguration::default();
        set_presentation_system(&mut options, Dx12SwapchainKind::DxgiFromHwnd);
        configure_for_platform(&mut options, None, Some(requested), true);
        assert_eq!(configured_presentation_system(&options), Some(requested));
    }
}

#[test]
fn non_windows_configuration_keeps_existing_presentation_system() {
    let mut options = WgpuConfiguration::default();
    set_presentation_system(&mut options, Dx12SwapchainKind::DxgiFromHwnd);
    let backends_before = configured_backends(&options);
    configure_for_platform(&mut options, None, Some(Dx12SwapchainKind::DxgiFromVisual), false);
    assert_eq!(configured_presentation_system(&options), Some(Dx12SwapchainKind::DxgiFromHwnd));
    assert_eq!(configured_backends(&options), backends_before);
}

fn set_presentation_system(options: &mut WgpuConfiguration, presentation: Dx12SwapchainKind) {
    if let WgpuSetup::CreateNew(create) = &mut options.wgpu_setup {
        create.instance_descriptor.backend_options.dx12.presentation_system = presentation;
    }
}

fn configured_presentation_system(options: &WgpuConfiguration) -> Option<Dx12SwapchainKind> {
    if let WgpuSetup::CreateNew(create) = &options.wgpu_setup {
        Some(create.instance_descriptor.backend_options.dx12.presentation_system)
    } else {
        None
    }
}

fn configured_backends(options: &WgpuConfiguration) -> Option<Backends> {
    if let WgpuSetup::CreateNew(create) = &options.wgpu_setup { Some(create.instance_descriptor.backends) } else { None }
}
