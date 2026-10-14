//! Базовые объекты Vulkan, которые живут всё время работы программы:
//! загрузчик, экземпляр (instance), поверхность окна (surface),
//! выбранная видеокарта (physical device), логическое устройство и очередь.
//! Объяснение по шагам: `docs/app/vulkan-init.md`.

use std::ffi::{CStr, c_char, c_void};

use anyhow::{Context as _, Result, anyhow};
use ash::{ext, khr, vk};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;

const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";

/// Всё, что нужно для работы с видеокартой, кроме цепочки показа (swapchain).
pub struct VulkanContext {
    /// Загруженная библиотека Vulkan. Хранится, чтобы она не выгрузилась раньше времени.
    _entry: ash::Entry,
    pub instance: ash::Instance,
    debug: Option<(ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub surface_fn: khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    pub physical_device: vk::PhysicalDevice,
    pub device: ash::Device,
    pub swapchain_fn: khr::swapchain::Device,
    /// Семейство очередей, умеющее и рисовать, и показывать картинку в окне.
    pub queue_family: u32,
    pub queue: vk::Queue,
    pub gpu_name: String,
}

impl VulkanContext {
    pub fn new(window: &Window) -> Result<Self> {
        // SAFETY: загружаем системную библиотеку Vulkan (vulkan-1.dll / libvulkan.so.1).
        let entry = unsafe { ash::Entry::load() }.context(
            "не найдена библиотека Vulkan. Обновите драйвер видеокарты: Vulkan входит в него",
        )?;

        let display = window.display_handle()?.as_raw();
        let validation = want_validation() && layer_available(&entry, VALIDATION_LAYER);

        let instance = create_instance(&entry, display, validation)?;
        let debug = validation
            .then(|| create_debug_messenger(&entry, &instance))
            .transpose()?;

        let surface_fn = khr::surface::Instance::new(&entry, &instance);
        // SAFETY: окно живёт дольше поверхности — Renderer уничтожается раньше окна.
        let surface = unsafe {
            ash_window::create_surface(
                &entry,
                &instance,
                display,
                window.window_handle()?.as_raw(),
                None,
            )
        }?;

        let (physical_device, queue_family) =
            pick_physical_device(&instance, &surface_fn, surface)?;
        let props = unsafe { instance.get_physical_device_properties(physical_device) };
        let gpu_name = props
            .device_name_as_c_str()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "неизвестная видеокарта".into());
        log::info!(
            "Видеокарта: {gpu_name} ({:?}), Vulkan {}.{}.{}",
            props.device_type,
            vk::api_version_major(props.api_version),
            vk::api_version_minor(props.api_version),
            vk::api_version_patch(props.api_version),
        );

        let priorities = [1.0];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(queue_family)
            .queue_priorities(&priorities)];
        let device_extensions = [khr::swapchain::NAME.as_ptr()];
        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_extension_names(&device_extensions);
        let device = unsafe { instance.create_device(physical_device, &device_info, None) }?;
        let queue = unsafe { device.get_device_queue(queue_family, 0) };
        let swapchain_fn = khr::swapchain::Device::new(&instance, &device);

        Ok(Self {
            _entry: entry,
            instance,
            debug,
            surface_fn,
            surface,
            physical_device,
            device,
            swapchain_fn,
            queue_family,
            queue,
            gpu_name,
        })
    }
}

impl Drop for VulkanContext {
    fn drop(&mut self) {
        // Порядок обратный созданию.
        unsafe {
            self.device.destroy_device(None);
            self.surface_fn.destroy_surface(self.surface, None);
            if let Some((loader, messenger)) = self.debug.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

/// Слои проверки включаются в отладочной сборке или по `RM_VALIDATION=1`.
fn want_validation() -> bool {
    cfg!(debug_assertions) || std::env::var("RM_VALIDATION").is_ok_and(|v| v == "1")
}

fn layer_available(entry: &ash::Entry, name: &CStr) -> bool {
    let layers = unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
    let found = layers
        .iter()
        .any(|l| l.layer_name_as_c_str().is_ok_and(|n| n == name));
    if !found {
        log::info!("Слои проверки Vulkan не установлены (это нормально без Vulkan SDK)");
    }
    found
}

fn create_instance(
    entry: &ash::Entry,
    display: winit::raw_window_handle::RawDisplayHandle,
    validation: bool,
) -> Result<ash::Instance> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"Reforged Medieval")
        .application_version(vk::make_api_version(0, 0, 1, 0))
        .engine_name(c"Reforged Medieval Engine")
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_1);

    // Расширения, без которых нельзя рисовать в окно этой ОС.
    let mut extensions: Vec<*const c_char> = ash_window::enumerate_required_extensions(display)
        .context("эта оконная система не поддерживается Vulkan")?
        .to_vec();
    let mut layers: Vec<*const c_char> = Vec::new();
    if validation {
        extensions.push(ext::debug_utils::NAME.as_ptr());
        layers.push(VALIDATION_LAYER.as_ptr());
        log::info!("Слои проверки Vulkan включены");
    }

    let info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extensions)
        .enabled_layer_names(&layers);
    unsafe { entry.create_instance(&info, None) }.context("не удалось создать экземпляр Vulkan")
}

fn create_debug_messenger(
    entry: &ash::Entry,
    instance: &ash::Instance,
) -> Result<(ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)> {
    let loader = ext::debug_utils::Instance::new(entry, instance);
    let info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(debug_callback));
    let messenger = unsafe { loader.create_debug_utils_messenger(&info, None) }?;
    Ok((loader, messenger))
}

/// Пересылает сообщения слоёв проверки в журнал программы.
unsafe extern "system" fn debug_callback(
    severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _kind: vk::DebugUtilsMessageTypeFlagsEXT,
    data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user: *mut c_void,
) -> vk::Bool32 {
    // SAFETY: Vulkan передаёт корректный указатель на время вызова.
    let msg = unsafe { data.as_ref() }
        .and_then(|d| unsafe { d.message_as_c_str() })
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();
    if severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
        log::error!("[Vulkan] {msg}");
    } else {
        log::warn!("[Vulkan] {msg}");
    }
    vk::FALSE
}

/// Насколько нам нравится тип видеокарты: дискретная лучше встроенной.
pub fn device_type_score(kind: vk::PhysicalDeviceType) -> u32 {
    match kind {
        vk::PhysicalDeviceType::DISCRETE_GPU => 4,
        vk::PhysicalDeviceType::INTEGRATED_GPU => 3,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
        vk::PhysicalDeviceType::CPU => 1,
        _ => 0,
    }
}

/// Выбирает видеокарту, которая умеет рисовать в наше окно.
fn pick_physical_device(
    instance: &ash::Instance,
    surface_fn: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32)> {
    let devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut best: Option<(u32, vk::PhysicalDevice, u32)> = None;
    for pd in devices {
        let Some(family) = find_queue_family(instance, surface_fn, surface, pd) else {
            continue;
        };
        if !supports_swapchain(instance, pd) {
            continue;
        }
        let score =
            device_type_score(unsafe { instance.get_physical_device_properties(pd) }.device_type);
        if best.is_none_or(|(s, ..)| score > s) {
            best = Some((score, pd, family));
        }
    }
    best.map(|(_, pd, f)| (pd, f))
        .ok_or_else(|| anyhow!("не найдена видеокарта с поддержкой Vulkan и вывода в окно"))
}

fn find_queue_family(
    instance: &ash::Instance,
    surface_fn: &khr::surface::Instance,
    surface: vk::SurfaceKHR,
    pd: vk::PhysicalDevice,
) -> Option<u32> {
    let families = unsafe { instance.get_physical_device_queue_family_properties(pd) };
    families.iter().enumerate().find_map(|(i, f)| {
        let i = i as u32;
        let graphics = f.queue_flags.contains(vk::QueueFlags::GRAPHICS);
        let present = unsafe { surface_fn.get_physical_device_surface_support(pd, i, surface) }
            .unwrap_or(false);
        (graphics && present).then_some(i)
    })
}

fn supports_swapchain(instance: &ash::Instance, pd: vk::PhysicalDevice) -> bool {
    unsafe { instance.enumerate_device_extension_properties(pd) }
        .unwrap_or_default()
        .iter()
        .any(|e| {
            e.extension_name_as_c_str()
                .is_ok_and(|n| n == khr::swapchain::NAME)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discrete_gpu_preferred() {
        assert!(
            device_type_score(vk::PhysicalDeviceType::DISCRETE_GPU)
                > device_type_score(vk::PhysicalDeviceType::INTEGRATED_GPU)
        );
        assert!(
            device_type_score(vk::PhysicalDeviceType::INTEGRATED_GPU)
                > device_type_score(vk::PhysicalDeviceType::CPU)
        );
    }
}
