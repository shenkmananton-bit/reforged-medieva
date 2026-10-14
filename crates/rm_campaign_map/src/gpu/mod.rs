//! Рисование карты через Vulkan.
//!
//! Карта рисуется внутри общего прохода рендера приложения тремя вызовами:
//!
//! 1. **Рельеф** — вся сетка одним вызовом. Пиксельный шейдер читает
//!    запечённые текстуры (H-02) и подмешивает цвет владельца региона из
//!    таблицы (H-04).
//! 2. **Маска границ** — та же сетка ещё раз, но без цвета: пиксели на
//!    границе двух стран записывают 1 в stencil-буфер (H-03).
//! 3. **Границы** — один треугольник на весь экран; stencil-тест
//!    пропускает только отмеченные пиксели, они и красятся.
//!
//! Приложение (rm_app) владеет окном, проходом рендера и буферами команд,
//! а этот модуль — только ресурсами карты. Подробно: `docs/campaign-map/render.md`.

mod memory;

use std::fmt;
use std::io::Cursor;

use ash::vk;

use crate::bake::BakedMap;
use crate::mesh::{TerrainMesh, TerrainVertex};
use crate::political::Political;
use memory::{GpuImage, HostBuffer};

/// Сколько регионов и фракций помещается в таблицу владельцев.
pub const TABLE_SIZE: usize = 256;

/// Ссылки на объекты Vulkan приложения, нужные карте.
#[derive(Clone, Copy)]
pub struct GpuRefs<'a> {
    pub instance: &'a ash::Instance,
    pub physical_device: vk::PhysicalDevice,
    pub device: &'a ash::Device,
    pub queue: vk::Queue,
    pub queue_family: u32,
}

#[derive(Debug)]
pub enum GpuError {
    Vulkan(vk::Result),
    NoMemoryType,
    Shader(std::io::Error),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vulkan(e) => write!(f, "ошибка Vulkan: {e}"),
            Self::NoMemoryType => write!(f, "нет подходящей видеопамяти для карты"),
            Self::Shader(e) => write!(f, "испорчен шейдер карты: {e}"),
        }
    }
}

impl std::error::Error for GpuError {}

impl From<vk::Result> for GpuError {
    fn from(e: vk::Result) -> Self {
        Self::Vulkan(e)
    }
}

/// Режим показа: готовая карта или отдельные запечённые слои (для отладки
/// и чтобы было видно, из чего карта собрана).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum ViewMode {
    Final = 0,
    Albedo = 1,
    Lightmap = 2,
    Normals = 3,
    Regions = 4,
    Political = 5,
}

impl ViewMode {
    pub const ALL: [ViewMode; 6] = [
        Self::Final,
        Self::Albedo,
        Self::Lightmap,
        Self::Normals,
        Self::Regions,
        Self::Political,
    ];

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&m| m == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Final => "карта",
            Self::Albedo => "цвет земли (albedo)",
            Self::Lightmap => "запечённые тени (lightmap)",
            Self::Normals => "запечённые нормали (normal map)",
            Self::Regions => "регионы",
            Self::Political => "политическая карта",
        }
    }
}

/// Флаги для [`MapConstants::ids`]`[3]`; совпадают с `map_common.glsl`.
pub const FLAG_FACTION_BORDERS: u32 = 1;
pub const FLAG_REGION_LINES: u32 = 2;
pub const FLAG_GLOW: u32 = 4;

/// Данные кадра для шейдеров карты — push-константы, ровно 128 байт.
/// Раскладка совпадает с `shaders/map_common.glsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapConstants {
    pub view_proj: [[f32; 4]; 4],
    /// xyz — позиция камеры, w — время в секундах.
    pub eye: [f32; 4],
    /// xyz — направление на солнце, w — сила окраски владельцев 0..1.
    pub sun: [f32; 4],
    /// x — сторона карты в мире, y — точек в текстурах, z — полуширина
    /// границы в пикселях, w — расстояние, с которого начинается дымка.
    pub map: [f32; 4],
    /// x — выбранный регион, y — регион под курсором, z — [`ViewMode`], w — флаги.
    pub ids: [u32; 4],
}

const _: () = assert!(std::mem::size_of::<MapConstants>() == 128);

/// Таблица владельцев в видеопамяти (std430, см. `map_common.glsl`).
#[repr(C)]
struct Tables {
    owner: [u32; TABLE_SIZE],
    colour: [[f32; 4]; TABLE_SIZE],
}

impl Tables {
    fn from(political: &Political) -> Self {
        let mut t = Tables {
            owner: [0; TABLE_SIZE],
            colour: [[0.5, 0.5, 0.5, 1.0]; TABLE_SIZE],
        };
        for (slot, &o) in t.owner.iter_mut().zip(&political.owner) {
            *slot = u32::from(o);
        }
        for (slot, f) in t.colour.iter_mut().zip(&political.factions) {
            let lin = |c: u8| (f32::from(c) / 255.0).powf(2.2);
            *slot = [lin(f.colour[0]), lin(f.colour[1]), lin(f.colour[2]), 1.0];
        }
        t
    }

    fn as_bytes(&self) -> &[u8] {
        // SAFETY: repr(C) из u32 и f32 без «дыр».
        unsafe {
            std::slice::from_raw_parts(
                self as *const Self as *const u8,
                std::mem::size_of::<Self>(),
            )
        }
    }
}

/// Все ресурсы карты в видеопамяти.
pub struct MapRenderer {
    images: Vec<GpuImage>,
    linear: vk::Sampler,
    nearest: vk::Sampler,
    vertices: HostBuffer,
    indices: HostBuffer,
    index_count: u32,
    /// Таблица владельцев — своя копия на каждый кадр в полёте, чтобы не
    /// переписать данные, которые видеокарта ещё читает.
    tables: Vec<HostBuffer>,
    set_layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    sets: Vec<vk::DescriptorSet>,
    layout: vk::PipelineLayout,
    terrain: vk::Pipeline,
    mask: vk::Pipeline,
    border: vk::Pipeline,
}

impl MapRenderer {
    /// Загружает карту в видеопамять и создаёт конвейеры для `render_pass`.
    /// Проход должен иметь вложение глубины со stencil.
    pub fn new(
        gpu: &GpuRefs,
        render_pass: vk::RenderPass,
        frames_in_flight: usize,
        baked: &BakedMap,
        mesh: &TerrainMesh,
    ) -> Result<Self, GpuError> {
        let device = gpu.device;
        let images = memory::upload_textures(
            gpu,
            &[
                (&baked.albedo, vk::Format::R8G8B8A8_SRGB),
                (&baked.normal, vk::Format::R8G8B8A8_UNORM),
                (&baked.light, vk::Format::R8G8_UNORM),
                (&baked.region, vk::Format::R8_UINT),
                (&baked.border, vk::Format::R8G8_UNORM),
            ],
        )?;

        let sampler = |filter: vk::Filter, mip: vk::SamplerMipmapMode| {
            let info = vk::SamplerCreateInfo::default()
                .mag_filter(filter)
                .min_filter(filter)
                .mipmap_mode(mip)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .max_lod(vk::LOD_CLAMP_NONE);
            unsafe { device.create_sampler(&info, None) }
        };
        let linear = sampler(vk::Filter::LINEAR, vk::SamplerMipmapMode::LINEAR)?;
        let nearest = sampler(vk::Filter::NEAREST, vk::SamplerMipmapMode::NEAREST)?;

        let vbytes = slice_bytes(&mesh.vertices);
        let mut vertices = HostBuffer::new(
            gpu,
            vbytes.len() as u64,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        vertices.write_bytes(0, vbytes);
        let ibytes = slice_bytes(&mesh.indices);
        let mut indices =
            HostBuffer::new(gpu, ibytes.len() as u64, vk::BufferUsageFlags::INDEX_BUFFER)?;
        indices.write_bytes(0, ibytes);

        let tables = (0..frames_in_flight)
            .map(|_| {
                HostBuffer::new(
                    gpu,
                    std::mem::size_of::<Tables>() as u64,
                    vk::BufferUsageFlags::STORAGE_BUFFER,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        // Набор привязок: 5 текстур и таблица владельцев.
        let stages = vk::ShaderStageFlags::FRAGMENT;
        let mut bindings: Vec<vk::DescriptorSetLayoutBinding> = (0..5)
            .map(|b| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(b)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(stages)
            })
            .collect();
        bindings.push(
            vk::DescriptorSetLayoutBinding::default()
                .binding(5)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(stages),
        );
        let set_layout = unsafe {
            device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        }?;
        let n = frames_in_flight as u32;
        let sizes = [
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                descriptor_count: 5 * n,
            },
            vk::DescriptorPoolSize {
                ty: vk::DescriptorType::STORAGE_BUFFER,
                descriptor_count: n,
            },
        ];
        let pool = unsafe {
            device.create_descriptor_pool(
                &vk::DescriptorPoolCreateInfo::default()
                    .max_sets(n)
                    .pool_sizes(&sizes),
                None,
            )
        }?;
        let layouts = vec![set_layout; frames_in_flight];
        let sets = unsafe {
            device.allocate_descriptor_sets(
                &vk::DescriptorSetAllocateInfo::default()
                    .descriptor_pool(pool)
                    .set_layouts(&layouts),
            )
        }?;
        for (set, table) in sets.iter().zip(&tables) {
            let image_infos: Vec<[vk::DescriptorImageInfo; 1]> = images
                .iter()
                .enumerate()
                .map(|(i, img)| {
                    // Номера регионов (3) читаются без фильтрации.
                    let s = if i == 3 { nearest } else { linear };
                    [vk::DescriptorImageInfo {
                        sampler: s,
                        image_view: img.view,
                        image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                    }]
                })
                .collect();
            let buffer_info = [vk::DescriptorBufferInfo {
                buffer: table.handle,
                offset: 0,
                range: vk::WHOLE_SIZE,
            }];
            let mut writes: Vec<vk::WriteDescriptorSet> = image_infos
                .iter()
                .enumerate()
                .map(|(i, info)| {
                    vk::WriteDescriptorSet::default()
                        .dst_set(*set)
                        .dst_binding(i as u32)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(info)
                })
                .collect();
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(*set)
                    .dst_binding(5)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(&buffer_info),
            );
            unsafe { device.update_descriptor_sets(&writes, &[]) };
        }

        let push = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: std::mem::size_of::<MapConstants>() as u32,
        }];
        let set_layouts = [set_layout];
        let layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default()
                    .set_layouts(&set_layouts)
                    .push_constant_ranges(&push),
                None,
            )
        }?;
        let [terrain, mask, border] = create_pipelines(device, render_pass, layout)?;

        Ok(Self {
            images,
            linear,
            nearest,
            vertices,
            indices,
            index_count: mesh.indices.len() as u32,
            tables,
            set_layout,
            pool,
            sets,
            layout,
            terrain,
            mask,
            border,
        })
    }

    /// Пересоздаёт конвейеры для нового прохода рендера (если у окна сменился формат).
    pub fn recreate_pipelines(
        &mut self,
        device: &ash::Device,
        render_pass: vk::RenderPass,
    ) -> Result<(), GpuError> {
        self.destroy_pipelines(device);
        [self.terrain, self.mask, self.border] =
            create_pipelines(device, render_pass, self.layout)?;
        Ok(())
    }

    /// Кладёт таблицу владельцев для кадра `frame`. Вызывать до записи команд кадра.
    pub fn write_tables(&mut self, frame: usize, political: &Political) {
        let t = Tables::from(political);
        self.tables[frame].write_bytes(0, t.as_bytes());
    }

    /// Записывает три прохода карты в буфер команд (внутри прохода рендера,
    /// после установки viewport и scissor).
    pub fn record(
        &self,
        device: &ash::Device,
        cmd: vk::CommandBuffer,
        frame: usize,
        constants: &MapConstants,
    ) {
        let bytes = struct_bytes(constants);
        unsafe {
            device.cmd_push_constants(
                cmd,
                self.layout,
                vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                0,
                bytes,
            );
            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[self.sets[frame]],
                &[],
            );
            device.cmd_bind_vertex_buffers(cmd, 0, &[self.vertices.handle], &[0]);
            device.cmd_bind_index_buffer(cmd, self.indices.handle, 0, vk::IndexType::UINT32);

            // 1. Рельеф с окраской владельцев.
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.terrain);
            device.cmd_draw_indexed(cmd, self.index_count, 1, 0, 0, 0);

            if constants.ids[3] & FLAG_FACTION_BORDERS != 0 {
                // 2. Маска границ в stencil.
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.mask);
                device.cmd_draw_indexed(cmd, self.index_count, 1, 0, 0, 0);
                // 3. Все границы одним полноэкранным треугольником.
                device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.border);
                device.cmd_draw(cmd, 3, 1, 0, 0);
            }
        }
    }

    fn destroy_pipelines(&mut self, device: &ash::Device) {
        unsafe {
            for p in [self.terrain, self.mask, self.border] {
                device.destroy_pipeline(p, None);
            }
        }
    }

    /// Освобождает видеопамять. Вызывать, когда видеокарта закончила все кадры.
    pub fn destroy(&mut self, device: &ash::Device) {
        self.destroy_pipelines(device);
        unsafe {
            device.destroy_pipeline_layout(self.layout, None);
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.set_layout, None);
            device.destroy_sampler(self.linear, None);
            device.destroy_sampler(self.nearest, None);
        }
        for t in &mut self.tables {
            t.destroy(device);
        }
        self.vertices.destroy(device);
        self.indices.destroy(device);
        for img in &mut self.images {
            img.destroy(device);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Pass {
    Terrain,
    Mask,
    Border,
}

fn create_pipelines(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
) -> Result<[vk::Pipeline; 3], GpuError> {
    let shader = |bytes: &[u8]| -> Result<vk::ShaderModule, GpuError> {
        let code = ash::util::read_spv(&mut Cursor::new(bytes)).map_err(GpuError::Shader)?;
        Ok(unsafe {
            device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
        }?)
    };
    let modules = [
        shader(include_bytes!("../../shaders/spv/terrain.vert.spv"))?,
        shader(include_bytes!("../../shaders/spv/terrain.frag.spv"))?,
        shader(include_bytes!("../../shaders/spv/border_mask.frag.spv"))?,
        shader(include_bytes!("../../shaders/spv/border.vert.spv"))?,
        shader(include_bytes!("../../shaders/spv/border.frag.spv"))?,
    ];
    let [terrain_vs, terrain_fs, mask_fs, border_vs, border_fs] = modules;
    let result = (|| {
        let t = create(
            device,
            render_pass,
            layout,
            terrain_vs,
            terrain_fs,
            Pass::Terrain,
        )?;
        let m = match create(device, render_pass, layout, terrain_vs, mask_fs, Pass::Mask) {
            Ok(m) => m,
            Err(e) => {
                unsafe { device.destroy_pipeline(t, None) };
                return Err(e);
            }
        };
        match create(
            device,
            render_pass,
            layout,
            border_vs,
            border_fs,
            Pass::Border,
        ) {
            Ok(b) => Ok([t, m, b]),
            Err(e) => {
                unsafe {
                    device.destroy_pipeline(t, None);
                    device.destroy_pipeline(m, None);
                }
                Err(e)
            }
        }
    })();
    for m in modules {
        unsafe { device.destroy_shader_module(m, None) };
    }
    result
}

fn create(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    vs: vk::ShaderModule,
    fs: vk::ShaderModule,
    pass: Pass,
) -> Result<vk::Pipeline, GpuError> {
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vs)
            .name(c"main"),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fs)
            .name(c"main"),
    ];
    let (bindings, attributes) = if pass == Pass::Border {
        (vec![], vec![])
    } else {
        (
            vec![vk::VertexInputBindingDescription {
                binding: 0,
                stride: std::mem::size_of::<TerrainVertex>() as u32,
                input_rate: vk::VertexInputRate::VERTEX,
            }],
            vec![
                vk::VertexInputAttributeDescription {
                    location: 0,
                    binding: 0,
                    format: vk::Format::R32G32B32_SFLOAT,
                    offset: 0,
                },
                vk::VertexInputAttributeDescription {
                    location: 1,
                    binding: 0,
                    format: vk::Format::R32G32_SFLOAT,
                    offset: 12,
                },
            ],
        )
    };
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(&attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);

    // Stencil: маска пишет 1 везде, где фрагмент не выброшен; проход
    // границ рисует только там, где записана 1.
    let write_one = vk::StencilOpState {
        fail_op: vk::StencilOp::KEEP,
        pass_op: vk::StencilOp::REPLACE,
        depth_fail_op: vk::StencilOp::KEEP,
        compare_op: vk::CompareOp::ALWAYS,
        compare_mask: 0xff,
        write_mask: 0xff,
        reference: 1,
    };
    let test_one = vk::StencilOpState {
        fail_op: vk::StencilOp::KEEP,
        pass_op: vk::StencilOp::KEEP,
        depth_fail_op: vk::StencilOp::KEEP,
        compare_op: vk::CompareOp::EQUAL,
        compare_mask: 0xff,
        write_mask: 0,
        reference: 1,
    };
    let depth = match pass {
        Pass::Terrain => vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(true)
            .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL),
        Pass::Mask => vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(true)
            .depth_write_enable(false)
            .depth_compare_op(vk::CompareOp::EQUAL)
            .stencil_test_enable(true)
            .front(write_one)
            .back(write_one),
        Pass::Border => vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(false)
            .depth_write_enable(false)
            .stencil_test_enable(true)
            .front(test_one)
            .back(test_one),
    };
    let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(pass == Pass::Border)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        // Маска не пишет цвет вовсе.
        .color_write_mask(if pass == Pass::Mask {
            vk::ColorComponentFlags::empty()
        } else {
            vk::ColorComponentFlags::RGBA
        })];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachment);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic = vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);

    let info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth)
        .color_blend_state(&blend)
        .dynamic_state(&dynamic)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0);
    let pipelines =
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &[info], None) }
            .map_err(|(_, e)| e)?;
    Ok(pipelines[0])
}

fn struct_bytes<T: Copy>(value: &T) -> &[u8] {
    // SAFETY: T — repr(C) из чисел без указателей и без «дыр».
    unsafe { std::slice::from_raw_parts(value as *const T as *const u8, std::mem::size_of::<T>()) }
}

fn slice_bytes<T: Copy>(data: &[T]) -> &[u8] {
    // SAFETY: T — repr(C) из чисел без указателей и без «дыр».
    unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, std::mem::size_of_val(data)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::political::FactionStyle;

    #[test]
    fn tables_match_shader_layout() {
        // std430: uint owner[256] (1024 байта), затем vec4 colour[256].
        assert_eq!(std::mem::size_of::<Tables>(), 1024 + 4096);
        assert_eq!(std::mem::offset_of!(Tables, colour), 1024);
    }

    #[test]
    fn tables_copy_owners_and_linear_colours() {
        let p = Political {
            factions: vec![FactionStyle {
                name: "x".into(),
                colour: [255, 0, 128],
            }],
            owner: vec![0, 0, 0],
            rebels: None,
            revision: 0,
        };
        let t = Tables::from(&p);
        assert_eq!(&t.owner[..3], &[0, 0, 0]);
        assert_eq!(t.colour[0][0], 1.0);
        assert_eq!(t.colour[0][1], 0.0);
        assert!((t.colour[0][2] - 0.22).abs() < 0.01, "sRGB → линейный");
    }

    #[test]
    fn view_modes_cycle() {
        let mut m = ViewMode::Final;
        for _ in 0..ViewMode::ALL.len() {
            m = m.next();
        }
        assert_eq!(m, ViewMode::Final);
    }
}
