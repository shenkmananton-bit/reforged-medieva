//! Графические конвейеры (pipelines): земля, модели, полупрозрачные метки, интерфейс.
//!
//! Шейдеры написаны на GLSL (`crates/rm_app/shaders/`) и заранее собраны
//! в SPIR-V (`shaders/spv/`), поэтому для сборки игры не нужен компилятор
//! шейдеров. Размеры окна задаются динамически, так что при изменении
//! размера конвейеры не пересоздаются.

use std::io::Cursor;

use anyhow::Result;
use ash::vk;

use super::UiRect;
use super::mesh::{Instance, Vertex};

/// Данные кадра для шейдеров мира (push-константы, см. `shaders/common.glsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameConstants {
    pub view_proj: [[f32; 4]; 4],
    /// xyz — направление на солнце, w — половина размера поля.
    pub sun: [f32; 4],
    /// xyz — позиция камеры, w — дистанция начала тумана.
    pub eye: [f32; 4],
    /// rgb — цвет тумана, a — длина перехода.
    pub fog: [f32; 4],
}

pub struct Pipelines {
    pub world_layout: vk::PipelineLayout,
    pub ui_layout: vk::PipelineLayout,
    pub ground: vk::Pipeline,
    pub mesh: vk::Pipeline,
    pub decal: vk::Pipeline,
    pub ui: vk::Pipeline,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ground,
    Mesh,
    Decal,
    Ui,
}

impl Pipelines {
    pub fn new(device: &ash::Device, render_pass: vk::RenderPass) -> Result<Self> {
        let world_range = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
            offset: 0,
            size: std::mem::size_of::<FrameConstants>() as u32,
        }];
        let world_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&world_range),
                None,
            )
        }?;
        let ui_range = [vk::PushConstantRange {
            stage_flags: vk::ShaderStageFlags::VERTEX,
            offset: 0,
            size: 8,
        }];
        let ui_layout = unsafe {
            device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().push_constant_ranges(&ui_range),
                None,
            )
        }?;

        let shader = |bytes: &[u8]| -> Result<vk::ShaderModule> {
            let code = ash::util::read_spv(&mut Cursor::new(bytes))?;
            Ok(unsafe {
                device
                    .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&code), None)
            }?)
        };
        let ground_vs = shader(include_bytes!("../../shaders/spv/ground.vert.spv"))?;
        let ground_fs = shader(include_bytes!("../../shaders/spv/ground.frag.spv"))?;
        let mesh_vs = shader(include_bytes!("../../shaders/spv/mesh.vert.spv"))?;
        let mesh_fs = shader(include_bytes!("../../shaders/spv/mesh.frag.spv"))?;
        let ui_vs = shader(include_bytes!("../../shaders/spv/ui.vert.spv"))?;
        let ui_fs = shader(include_bytes!("../../shaders/spv/ui.frag.spv"))?;

        let build = |kind: Kind, vs, fs, layout| create(device, render_pass, layout, vs, fs, kind);
        let result = (|| {
            Ok(Self {
                world_layout,
                ui_layout,
                ground: build(Kind::Ground, ground_vs, ground_fs, world_layout)?,
                mesh: build(Kind::Mesh, mesh_vs, mesh_fs, world_layout)?,
                decal: build(Kind::Decal, mesh_vs, mesh_fs, world_layout)?,
                ui: build(Kind::Ui, ui_vs, ui_fs, ui_layout)?,
            })
        })();
        // Модули нужны только на время создания конвейеров.
        for m in [ground_vs, ground_fs, mesh_vs, mesh_fs, ui_vs, ui_fs] {
            unsafe { device.destroy_shader_module(m, None) };
        }
        result
    }

    pub fn destroy(&mut self, device: &ash::Device) {
        unsafe {
            for p in [self.ground, self.mesh, self.decal, self.ui] {
                device.destroy_pipeline(p, None);
            }
            device.destroy_pipeline_layout(self.world_layout, None);
            device.destroy_pipeline_layout(self.ui_layout, None);
        }
    }
}

fn create(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    vs: vk::ShaderModule,
    fs: vk::ShaderModule,
    kind: Kind,
) -> Result<vk::Pipeline> {
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

    // Входы вершинного шейдера.
    let f4 = vk::Format::R32G32B32A32_SFLOAT;
    let f3 = vk::Format::R32G32B32_SFLOAT;
    let attr = |location, binding, format, offset| vk::VertexInputAttributeDescription {
        location,
        binding,
        format,
        offset,
    };
    let (bindings, attributes): (Vec<_>, Vec<_>) = match kind {
        Kind::Ground => (vec![], vec![]),
        Kind::Mesh | Kind::Decal => (
            vec![
                vk::VertexInputBindingDescription {
                    binding: 0,
                    stride: std::mem::size_of::<Vertex>() as u32,
                    input_rate: vk::VertexInputRate::VERTEX,
                },
                vk::VertexInputBindingDescription {
                    binding: 1,
                    stride: std::mem::size_of::<Instance>() as u32,
                    input_rate: vk::VertexInputRate::INSTANCE,
                },
            ],
            vec![
                attr(0, 0, f3, 0),
                attr(1, 0, f3, 12),
                attr(2, 0, f4, 24),
                attr(3, 1, f4, 0),
                attr(4, 1, f4, 16),
                attr(5, 1, f4, 32),
                attr(6, 1, f4, 48),
            ],
        ),
        Kind::Ui => (
            vec![vk::VertexInputBindingDescription {
                binding: 0,
                stride: std::mem::size_of::<UiRect>() as u32,
                input_rate: vk::VertexInputRate::INSTANCE,
            }],
            vec![attr(0, 0, f4, 0), attr(1, 0, f4, 16)],
        ),
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
    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(kind != Kind::Ui)
        .depth_write_enable(matches!(kind, Kind::Ground | Kind::Mesh))
        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
    let blend_on = matches!(kind, Kind::Decal | Kind::Ui);
    let blend_attachment = [vk::PipelineColorBlendAttachmentState::default()
        .blend_enable(blend_on)
        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
        .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .color_blend_op(vk::BlendOp::ADD)
        .src_alpha_blend_factor(vk::BlendFactor::ONE)
        .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
        .alpha_blend_op(vk::BlendOp::ADD)
        .color_write_mask(vk::ColorComponentFlags::RGBA)];
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
