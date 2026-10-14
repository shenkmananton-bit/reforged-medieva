//! Рендер: всё, что связано с Vulkan.
//!
//! Каждый кадр рисуется за один проход рендера в три слоя:
//! 1. земля (один большой прямоугольник, трава считается в шейдере);
//! 2. модели солдат, коней, стрел и знамён — по одному вызову на тип модели
//!    (GPU instancing, хак H-13), затем полупрозрачные метки на земле;
//! 3. интерфейс — все прямоугольники и буквы одним вызовом (хак H-25).
//!
//! В режиме карты кампании слой 1 заменяется картой из `rm_campaign_map`
//! (её ресурсы и конвейеры живут там, здесь только вызов).
//!
//! Объяснение: `docs/app/vulkan-init.md`, `docs/app/frame.md`, `docs/app/render.md`.

mod context;
mod gpu;
pub mod mesh;
mod pipelines;
mod swapchain;

use anyhow::{Result, bail};
use ash::vk;
use rm_campaign_map::gpu::{GpuRefs, MapConstants, MapRenderer};
use rm_campaign_map::{BakedMap, Political, TerrainMesh};
use winit::window::Window;

use context::VulkanContext;
use gpu::{DepthImage, HostBuffer};
use mesh::{Instance, MESH_COUNT, MeshRange};
pub use pipelines::FrameConstants;
use pipelines::Pipelines;
use swapchain::Swapchain;

/// Сколько экземпляров моделей помещается в кадр (солдаты, кони, стрелы, метки).
const MAX_INSTANCES: u64 = 120_000;
/// Сколько прямоугольников интерфейса помещается в кадр.
const MAX_UI_RECTS: u64 = 60_000;

/// Прямоугольник интерфейса в пикселях окна. Раскладка совпадает с `ui.vert`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiRect {
    /// x, y (левый верхний угол), ширина, высота.
    pub rect: [f32; 4],
    pub color: [f32; 4],
}

/// Всё, что нужно нарисовать в одном кадре.
pub struct Frame<'a> {
    /// Цвет неба.
    pub clear: [f32; 4],
    pub constants: FrameConstants,
    /// Непрозрачные модели, по списку экземпляров на каждый тип модели.
    pub meshes: &'a [Vec<Instance>; MESH_COUNT],
    /// Полупрозрачные круги на земле (тени, выделение, точки назначения).
    pub decals: &'a [Instance],
    pub ui: &'a [UiRect],
    /// Карта кампании вместо земли битвы (если загружена, см. [`Renderer::load_campaign_map`]).
    pub campaign: Option<CampaignFrame<'a>>,
}

/// Что карте кампании нужно в кадре.
pub struct CampaignFrame<'a> {
    pub constants: MapConstants,
    pub political: &'a Political,
}

/// Сколько кадров CPU может готовить, пока GPU рисует предыдущие.
/// 2 — стандартный баланс между задержкой ввода и загрузкой GPU.
const FRAMES_IN_FLIGHT: usize = 2;

/// Что получилось при попытке нарисовать кадр.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// Кадр нарисован и отправлен на экран.
    Presented,
    /// Окно свёрнуто или цепочка устарела — кадр пропущен.
    Skipped,
}

/// Ресурсы одного «кадра в полёте».
struct FrameSync {
    command_buffer: vk::CommandBuffer,
    /// Экземпляры моделей и интерфейс этого кадра (у каждого кадра в полёте свои,
    /// чтобы не переписать данные, которые GPU ещё читает).
    instances: HostBuffer,
    ui: HostBuffer,
    /// GPU сигналит, когда картинка из цепочки свободна для рисования.
    image_available: vk::Semaphore,
    /// CPU ждёт этот забор, прежде чем снова использовать буфер команд кадра.
    in_flight: vk::Fence,
}

pub struct Renderer {
    // Порядок важен только для чтения: уничтожение делает Drop вручную.
    ctx: VulkanContext,
    swapchain: Swapchain,
    depth_format: vk::Format,
    depth: DepthImage,
    render_pass: vk::RenderPass,
    pipelines: Pipelines,
    framebuffers: Vec<vk::Framebuffer>,
    /// Все модели в одном буфере вершин.
    vertices: HostBuffer,
    mesh_ranges: [MeshRange; MESH_COUNT],
    /// По одному семафору «рисование закончено» на каждую картинку цепочки:
    /// так показ картинки не путается с соседними кадрами.
    render_finished: Vec<vk::Semaphore>,
    command_pool: vk::CommandPool,
    frames: Vec<FrameSync>,
    frame_index: usize,
    vsync: bool,
    window_size: (u32, u32),
    needs_recreate: bool,
    /// Ресурсы карты кампании, если включён её режим.
    map: Option<MapRenderer>,
}

impl Renderer {
    pub fn new(window: &Window, vsync: bool) -> Result<Self> {
        let ctx = VulkanContext::new(window)?;
        let size = window.inner_size();
        let window_size = (size.width, size.height);
        let swapchain = Swapchain::new(&ctx, window_size, vsync, vk::SwapchainKHR::null())?;
        let depth_format = gpu::pick_depth_format(&ctx)?;
        let depth = DepthImage::new(&ctx, depth_format, swapchain.extent)?;
        let render_pass = create_render_pass(&ctx.device, swapchain.format, depth_format)?;
        let pipelines = Pipelines::new(&ctx.device, render_pass)?;
        let framebuffers = create_framebuffers(&ctx.device, render_pass, &swapchain, depth.view)?;
        let (verts, mesh_ranges) = mesh::build_all();
        let mut vertices = HostBuffer::new(
            &ctx,
            std::mem::size_of_val(verts.as_slice()) as u64,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        vertices.write(0, &verts);
        let render_finished = create_semaphores(&ctx.device, swapchain.images.len())?;

        let pool_info = vk::CommandPoolCreateInfo::default()
            .queue_family_index(ctx.queue_family)
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
        let command_pool = unsafe { ctx.device.create_command_pool(&pool_info, None) }?;
        let alloc = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(FRAMES_IN_FLIGHT as u32);
        let buffers = unsafe { ctx.device.allocate_command_buffers(&alloc) }?;
        let image_available = create_semaphores(&ctx.device, FRAMES_IN_FLIGHT)?;
        let frames = buffers
            .into_iter()
            .zip(image_available)
            .map(|(command_buffer, image_available)| {
                // Забор создаётся «поднятым», чтобы первый кадр не ждал вечно.
                let info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
                let in_flight = unsafe { ctx.device.create_fence(&info, None) }?;
                let instances = HostBuffer::new(
                    &ctx,
                    MAX_INSTANCES * std::mem::size_of::<Instance>() as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                )?;
                let ui = HostBuffer::new(
                    &ctx,
                    MAX_UI_RECTS * std::mem::size_of::<UiRect>() as u64,
                    vk::BufferUsageFlags::VERTEX_BUFFER,
                )?;
                Ok(FrameSync {
                    command_buffer,
                    instances,
                    ui,
                    image_available,
                    in_flight,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self {
            ctx,
            swapchain,
            depth_format,
            depth,
            render_pass,
            pipelines,
            framebuffers,
            vertices,
            mesh_ranges,
            render_finished,
            command_pool,
            frames,
            frame_index: 0,
            vsync,
            window_size,
            needs_recreate: false,
            map: None,
        })
    }

    /// Загружает карту кампании в видеопамять. После этого кадры с
    /// [`Frame::campaign`] рисуют карту вместо земли битвы.
    pub fn load_campaign_map(&mut self, baked: &BakedMap, mesh: &TerrainMesh) -> Result<()> {
        if !gpu::has_stencil(self.depth_format) {
            bail!("видеокарта не дала буфер глубины со stencil, а он нужен для границ на карте");
        }
        let refs = GpuRefs {
            instance: &self.ctx.instance,
            physical_device: self.ctx.physical_device,
            device: &self.ctx.device,
            queue: self.ctx.queue,
            queue_family: self.ctx.queue_family,
        };
        let map = MapRenderer::new(&refs, self.render_pass, FRAMES_IN_FLIGHT, baked, mesh)?;
        if let Some(mut old) = self.map.replace(map) {
            unsafe { self.ctx.device.device_wait_idle() }?;
            old.destroy(&self.ctx.device);
        }
        Ok(())
    }

    pub fn gpu_name(&self) -> &str {
        &self.ctx.gpu_name
    }

    /// Сообщает рендеру новый размер окна; цепочка пересоздастся перед следующим кадром.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.window_size = (width, height);
        self.needs_recreate = true;
    }

    /// Размер области рисования в пикселях.
    pub fn extent(&self) -> (u32, u32) {
        (self.swapchain.extent.width, self.swapchain.extent.height)
    }

    /// Рисует один кадр.
    pub fn draw_frame(&mut self, scene: &Frame) -> Result<FrameOutcome> {
        // Свёрнутое окно имеет размер 0×0: рисовать некуда.
        if self.window_size.0 == 0 || self.window_size.1 == 0 {
            return Ok(FrameOutcome::Skipped);
        }
        if self.needs_recreate {
            self.recreate_swapchain()?;
        }

        let device = &self.ctx.device;
        let frame = &mut self.frames[self.frame_index];
        let campaign = scene.campaign.as_ref().filter(|_| self.map.is_some());

        // 1. Ждём, пока GPU закончит кадр, который рисовался в этом слоте раньше.
        unsafe { device.wait_for_fences(&[frame.in_flight], true, u64::MAX) }?;

        // 2. Берём свободную картинку из цепочки.
        let acquired = unsafe {
            self.ctx.swapchain_fn.acquire_next_image(
                self.swapchain.handle,
                u64::MAX,
                frame.image_available,
                vk::Fence::null(),
            )
        };
        let image_index = match acquired {
            Ok((index, suboptimal)) => {
                self.needs_recreate |= suboptimal;
                index
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                self.needs_recreate = true;
                return Ok(FrameOutcome::Skipped);
            }
            Err(e) => return Err(e.into()),
        };
        // Забор опускаем только когда точно отправим работу, иначе следующий кадр зависнет.
        unsafe { device.reset_fences(&[frame.in_flight]) }?;

        // 3. Кладём экземпляры этого кадра в его буферы и записываем команды.
        let mut draws = [(0u32, 0u32); MESH_COUNT];
        let mut written = 0u64;
        let isize = std::mem::size_of::<Instance>() as u64;
        for (i, list) in scene.meshes.iter().enumerate() {
            let n = frame.instances.write(written * isize, list) as u32;
            draws[i] = (written as u32, n);
            written += u64::from(n);
        }
        let decal_first = written as u32;
        let decal_count = frame.instances.write(written * isize, scene.decals) as u32;
        let ui_count = frame.ui.write(0, scene.ui) as u32;
        if let (Some(map), Some(c)) = (self.map.as_mut(), campaign) {
            map.write_tables(self.frame_index, c.political);
        }

        let cmd = frame.command_buffer;
        let extent = self.swapchain.extent;
        unsafe {
            device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            let begin = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            device.begin_command_buffer(cmd, &begin)?;
            let clear = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: scene.clear,
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.0,
                        stencil: 0,
                    },
                },
            ];
            let pass = vk::RenderPassBeginInfo::default()
                .render_pass(self.render_pass)
                .framebuffer(self.framebuffers[image_index as usize])
                .render_area(vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                })
                .clear_values(&clear);
            device.cmd_begin_render_pass(cmd, &pass, vk::SubpassContents::INLINE);
            device.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: extent.width as f32,
                    height: extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D::default(),
                    extent,
                }],
            );

            // Слой 1: земля или карта кампании.
            if let (Some(map), Some(c)) = (&self.map, campaign) {
                map.record(device, cmd, self.frame_index, &c.constants);
            }
            let constants = as_bytes(&scene.constants);
            device.cmd_push_constants(
                cmd,
                self.pipelines.world_layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                constants,
            );
            if campaign.is_none() {
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipelines.ground,
                );
                device.cmd_draw(cmd, 6, 1, 0, 0);
            }

            // Слой 2: модели, по одному вызову на тип (instancing), потом метки.
            device.cmd_bind_vertex_buffers(
                cmd,
                0,
                &[self.vertices.handle, frame.instances.handle],
                &[0, 0],
            );
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipelines.mesh);
            for (i, &(first, count)) in draws.iter().enumerate() {
                if count > 0 && i != mesh::MeshId::Disc as usize {
                    let r = self.mesh_ranges[i];
                    device.cmd_draw(cmd, r.count, count, r.first, first);
                }
            }
            if decal_count > 0 {
                let r = self.mesh_ranges[mesh::MeshId::Disc as usize];
                device.cmd_bind_pipeline(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipelines.decal,
                );
                device.cmd_draw(cmd, r.count, decal_count, r.first, decal_first);
            }

            // Слой 3: интерфейс.
            if ui_count > 0 {
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.pipelines.ui);
                let screen = [extent.width as f32, extent.height as f32];
                device.cmd_push_constants(
                    cmd,
                    self.pipelines.ui_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    as_bytes(&screen),
                );
                device.cmd_bind_vertex_buffers(cmd, 0, &[frame.ui.handle], &[0]);
                device.cmd_draw(cmd, 6, ui_count, 0, 0);
            }

            device.cmd_end_render_pass(cmd);
            device.end_command_buffer(cmd)?;
        }

        // 4. Отправляем команды в очередь GPU.
        let render_finished = self.render_finished[image_index as usize];
        let wait = [frame.image_available];
        let stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let signal = [render_finished];
        let cmds = [cmd];
        let submit = vk::SubmitInfo::default()
            .wait_semaphores(&wait)
            .wait_dst_stage_mask(&stages)
            .command_buffers(&cmds)
            .signal_semaphores(&signal);
        unsafe { device.queue_submit(self.ctx.queue, &[submit], frame.in_flight) }?;

        // 5. Просим систему показать картинку, когда рисование закончится.
        let swapchains = [self.swapchain.handle];
        let indices = [image_index];
        let present = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal)
            .swapchains(&swapchains)
            .image_indices(&indices);
        match unsafe {
            self.ctx
                .swapchain_fn
                .queue_present(self.ctx.queue, &present)
        } {
            Ok(suboptimal) => self.needs_recreate |= suboptimal,
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => self.needs_recreate = true,
            Err(e) => return Err(e.into()),
        }

        self.frame_index = (self.frame_index + 1) % FRAMES_IN_FLIGHT;
        Ok(FrameOutcome::Presented)
    }

    fn recreate_swapchain(&mut self) -> Result<()> {
        let device = &self.ctx.device;
        // Простой и надёжный путь: дождаться, пока GPU всё доделает.
        unsafe { device.device_wait_idle() }?;
        destroy_framebuffers(device, &mut self.framebuffers);
        destroy_semaphores(device, &mut self.render_finished);
        self.depth.destroy(device);

        let old = self.swapchain.handle;
        let new = Swapchain::new(&self.ctx, self.window_size, self.vsync, old)?;
        let mut old_swapchain = std::mem::replace(&mut self.swapchain, new);
        old_swapchain.destroy(&self.ctx);

        self.depth = DepthImage::new(&self.ctx, self.depth_format, self.swapchain.extent)?;
        // Формат обычно тот же, но если сменился — нужны новые проход рендера и конвейеры.
        if self.swapchain.format != old_swapchain.format {
            self.pipelines.destroy(device);
            unsafe { device.destroy_render_pass(self.render_pass, None) };
            self.render_pass =
                create_render_pass(device, self.swapchain.format, self.depth_format)?;
            self.pipelines = Pipelines::new(device, self.render_pass)?;
            if let Some(map) = self.map.as_mut() {
                map.recreate_pipelines(device, self.render_pass)?;
            }
        }
        self.framebuffers =
            create_framebuffers(device, self.render_pass, &self.swapchain, self.depth.view)?;
        self.render_finished = create_semaphores(device, self.swapchain.images.len())?;
        self.needs_recreate = false;
        log::debug!("Swapchain пересоздан: {:?}", self.swapchain.extent);
        Ok(())
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        let device = &self.ctx.device;
        unsafe {
            let _ = device.device_wait_idle();
            for f in &mut self.frames {
                device.destroy_fence(f.in_flight, None);
                device.destroy_semaphore(f.image_available, None);
                f.instances.destroy(device);
                f.ui.destroy(device);
            }
            self.vertices.destroy(device);
            if let Some(map) = self.map.as_mut() {
                map.destroy(device);
            }
            self.pipelines.destroy(device);
            self.depth.destroy(device);
            device.destroy_command_pool(self.command_pool, None);
            destroy_semaphores(device, &mut self.render_finished);
            destroy_framebuffers(device, &mut self.framebuffers);
            device.destroy_render_pass(self.render_pass, None);
        }
        self.swapchain.destroy(&self.ctx);
        // VulkanContext уничтожит устройство и экземпляр в своём Drop.
    }
}

/// Проход рендера: цвет (картинка окна) и глубина. В начале оба очищаются,
/// в конце цвет переводится в состояние «готов к показу».
fn create_render_pass(
    device: &ash::Device,
    format: vk::Format,
    depth_format: vk::Format,
) -> Result<vk::RenderPass> {
    let attachments = [
        vk::AttachmentDescription::default()
            .format(format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::PRESENT_SRC_KHR),
        vk::AttachmentDescription::default()
            .format(depth_format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::CLEAR)
            .store_op(vk::AttachmentStoreOp::DONT_CARE)
            // Stencil очищается каждый кадр: им пользуется карта кампании (H-03).
            .stencil_load_op(vk::AttachmentLoadOp::CLEAR)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
    ];
    let color_ref = [vk::AttachmentReference {
        attachment: 0,
        layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    }];
    let depth_ref = vk::AttachmentReference {
        attachment: 1,
        layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
    };
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_ref)
        .depth_stencil_attachment(&depth_ref)];
    // Не начинать запись в картинку, пока система не отдала её (семафор image_available),
    // и не очищать глубину, пока предыдущий кадр её ещё читает.
    let stages = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
        | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
        | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
    let dependencies = [vk::SubpassDependency {
        src_subpass: vk::SUBPASS_EXTERNAL,
        dst_subpass: 0,
        src_stage_mask: stages,
        dst_stage_mask: stages,
        src_access_mask: vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
        dst_access_mask: vk::AccessFlags::COLOR_ATTACHMENT_WRITE
            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE
            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ,
        dependency_flags: vk::DependencyFlags::empty(),
    }];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    Ok(unsafe { device.create_render_pass(&info, None) }?)
}

fn create_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    swapchain: &Swapchain,
    depth_view: vk::ImageView,
) -> Result<Vec<vk::Framebuffer>> {
    swapchain
        .views
        .iter()
        .map(|&view| {
            let attachments = [view, depth_view];
            let info = vk::FramebufferCreateInfo::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(swapchain.extent.width)
                .height(swapchain.extent.height)
                .layers(1);
            Ok(unsafe { device.create_framebuffer(&info, None) }?)
        })
        .collect()
}

/// Байты простой структуры для push-констант.
fn as_bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: T — repr(C) структура из f32 без указателей и без «дыр».
    unsafe { std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>()) }
}

fn create_semaphores(device: &ash::Device, count: usize) -> Result<Vec<vk::Semaphore>> {
    (0..count)
        .map(|_| Ok(unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) }?))
        .collect()
}

fn destroy_framebuffers(device: &ash::Device, list: &mut Vec<vk::Framebuffer>) {
    for fb in list.drain(..) {
        unsafe { device.destroy_framebuffer(fb, None) };
    }
}

fn destroy_semaphores(device: &ash::Device, list: &mut Vec<vk::Semaphore>) {
    for s in list.drain(..) {
        unsafe { device.destroy_semaphore(s, None) };
    }
}
