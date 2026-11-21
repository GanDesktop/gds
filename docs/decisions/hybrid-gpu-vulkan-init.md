# Hybrid-GPU Direct Vulkan Initialization

On Wayland hybrid-graphics laptops where the compositor renders on an integrated GPU (iGPU) while a discrete GPU (dGPU) sits in runtime power suspension (`runtime_status = suspended`), standard `wgpu` initialization wakes the sleeping dGPU and stalls cold first-frame startup by roughly two seconds. GDS avoids this delay inside its `GanDesktop/zgui` (`gds-0.1.0`) fork without changing rendering semantics or leaking process-wide driver filters.

## Problem

In `zgui-render-wgpu`, standard `wgpu::Instance::new` initializes `Backends::VULKAN | Backends::GL` before adapter selection or surface testing runs. Inside `wgpu-hal`, Vulkan extension discovery queries global instance extensions from the Vulkan Loader prior to `vkCreateInstance`, forcing the Loader to load and probe every installed Vulkan ICD (including the proprietary NVIDIA driver) and waking a suspended dGPU before adapter sorting ever executes.

Because the wake occurs inside `wgpu::Instance::new`, post-enumeration adapter sorting, `ZGUI_DEVICE_ID`, `PowerPreference::LowPower`, disabling GL, and filtering EGL are all ineffective.

## Rejected Alternatives

- **Process-wide `VK_DRIVER_FILES` filtering**:
  - Calling `std::env::set_var` in Rust after the Tokio `Supervisor` starts is undefined behavior under multi-threaded Unix environments.
  - Injecting `VK_DRIVER_FILES` in application launchers is inherited by every child process spawned by embedded terminals (`gds-files`) and "Open With" external applications, hiding the dGPU from user workloads that legitimately require it.
- **Defaulting to OpenGL (`GL-first`)**:
  - EGL/GLES fallback lacks Vulkan presentation flexibility, serializes context access, and loses device-ID-based GPU matching.

## Mechanism & Safety Invariants

The optimization splits **compositor DRM identity propagation** (`zgui-platform-wayland`) from **allowlisted exclusive Vulkan initialization and clean two-stage fallback** (`zgui-render-wgpu` and `zgui/src/app/graphics.rs`):

1. **Single-Roundtrip Compositor DRM Identity (`zgui-platform-wayland`)**:
   - Binds `zwp_linux_dmabuf_v1` (`v4..=5`) and requests `get_default_feedback` on the primary Wayland connection before the initial registry roundtrip, retaining `drm_render_dev` (`dev_t`), `vendor_id`, and `device_id` in `CompositorGpuHint` across windows and recovery.

2. **Precondition Gates (`sysfs` + Environment Check)**:
   - Bypass the fast path whenever any explicit GPU, loader, or layer override environment variable is non-empty:
     - `ZGUI_DEVICE_ID`, `ZED_DEVICE_ID`, `ZGUI_BACKENDS`, `WGPU_BACKEND`, `WGPU_ADAPTER_NAME`
     - `DRI_PRIME`, `__NV_PRIME_RENDER_OFFLOAD` (non-empty and not `"0"`)
     - `VK_DRIVER_FILES`, `VK_ADD_DRIVER_FILES`, `VK_ICD_FILENAMES`, `VK_LOADER_DRIVERS_SELECT`, `VK_LOADER_DRIVERS_DISABLE`, `VK_INSTANCE_LAYERS`
   - Resolve `drm_render_dev` (`major:minor`) through `/sys/dev/char/{major}:{minor}/device` to its canonical PCI device path and bound kernel driver symlink (`device/driver`).
   - Scan `/sys/class/drm/card*/device` and enter the fast path **only** when at least one distinct PCI device with a different vendor ID reports `power/runtime_status` in `{"suspended", "suspending"}`. Single-GPU desktops, VMs, and already-active dGPU systems run standard `wgpu` initialization untouched.

3. **Conservative Allowlist ICD Resolution**:
   - `(0x1002, "amdgpu") -> libvulkan_radeon.so` (Mesa RADV; abort fast path if competing 64-bit `amdvlk` or `amd_icd` manifests are present).
   - `(0x8086, "i915" | "xe") -> libvulkan_intel.so` (Mesa ANV; tolerates Mesa's co-installed `intel_hasvk_icd` manifest).
   - Search standard Vulkan ICD directories (`$XDG_CONFIG_HOME/vulkan/icd.d`, `/etc/vulkan/icd.d`, `/run/opengl-driver/share/vulkan/icd.d`, `$XDG_DATA_DIRS/vulkan/icd.d`, `/usr/local/share/vulkan/icd.d`, `/usr/share/vulkan/icd.d`), reject non-64-bit manifests on 64-bit targets, canonicalize `library_path`, require a single unique match, and verify ELF64 headers (`\x7fELF`, `EI_CLASS == 2`).

4. **Exclusive Instance Construction & Two-Stage Fallback (`zgui-render-wgpu`, `zgui/src/app/graphics.rs`)**:
   - Load the verified ICD directly, query driver extensions via `wgpu::hal::vulkan::Instance::desired_extensions` without probing unrelated ICDs, create the instance with `VK_LUNARG_direct_driver_loading` in `VK_DIRECT_DRIVER_LOADING_MODE_EXCLUSIVE_LUNARG`, and wrap it via `wgpu::hal::vulkan::Instance::from_raw` / `wgpu::Instance::from_hal` with clean drop ordering.
   - Verify adapter creation, surface configuration, and matching `compositor_gpu` (`vendor_id` and `device_id`); on any `Err`, drop the exclusive instance and fall back cleanly to standard `VULKAN | GL` initialization across initial startup and `device_lost` recovery.

## Retirement Condition

Retire this fast path once upstream `zortax/zgui` and `wgpu` ([gfx-rs/wgpu#9912](https://github.com/gfx-rs/wgpu/issues/9912)) / Vulkan Loader support targeted DRM-device initialization without probing unrelated suspended GPUs.
