mod utils;

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{ImageData, WebGlProgram, WebGlRenderingContext, WebGlShader};

const WIDTH: i32 = 128;
const HEIGHT: i32 = 128;
const CHANNELS: i32 = 4;
const BUFFER_SIZE: usize = ((WIDTH * HEIGHT) * CHANNELS) as usize;
static mut PIXEL_DATA: [u8; BUFFER_SIZE] = [255; BUFFER_SIZE];
static mut PIXEL_DATA_UPDATING: bool = false;
static mut PIXEL_DATA_UPDATED: bool = false;
// static mut CLIENT_READY: bool = false;
const VERTICES: [f32; 18] = [
    -1.0, -1.0, 0.0, // Bottom left
    1.0, -1.0, 0.0, // Bottem right
    1.0, 1.0, 0.0, // Top right
    -1.0, -1.0, 0.0, // Bottom left
    1.0, 1.0, 0.0, // Top right
    -1.0, 1.0, 0.0, // Top left
];

fn window() -> web_sys::Window {
    web_sys::window().expect("no global `window` exists")
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("should register `requestAnimationFrame` OK");
}

#[wasm_bindgen(start)]
pub fn start() {
    utils::set_panic_hook();
    log!("Hello there! Compositor canvas starting/loading");
}

#[wasm_bindgen]
pub fn initialise(element_id: String) -> Result<(), JsValue> {
    log!(
        "Compositor canvas (element_id: String = `{}`) initialisation",
        &element_id
    );

    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document.get_element_by_id(&element_id).unwrap();
    let canvas: web_sys::HtmlCanvasElement = canvas.dyn_into::<web_sys::HtmlCanvasElement>()?;

    let context = canvas
        .get_context("webgl")?
        .unwrap()
        .dyn_into::<WebGlRenderingContext>()?;

    let vert_shader = compile_shader(
        &context,
        WebGlRenderingContext::VERTEX_SHADER,
        r#"
        attribute vec4 position;
        attribute vec2 textureCoord;

        varying highp vec2 vTextureCoord;

        void main(void) {
            gl_Position = position;
            vTextureCoord = textureCoord;
        }
    "#,
    )?;
    let frag_shader = compile_shader(
        &context,
        WebGlRenderingContext::FRAGMENT_SHADER,
        r#"
        varying highp vec2 vTextureCoord;

        uniform sampler2D image;

        void main(void) {
            gl_FragColor = texture2D(image, vTextureCoord);
            gl_FragColor = vec4(gl_FragColor.b, gl_FragColor.g, gl_FragColor.r, gl_FragColor.a);
        }
    "#,
    )?;
    let program = link_program(&context, &vert_shader, &frag_shader)?;
    let position_location = context.get_attrib_location(&program, "position");
    let texcoord_location = context.get_attrib_location(&program, "textureCoord");
    let texture_location = context.get_uniform_location(&program, "image"); //.unwrap();

    // Bind shader
    context.use_program(Some(&program));

    // Build model

    let vertex_buffer = context
        .create_buffer()
        .ok_or("failed to create vertex buffer")?;
    context.bind_buffer(WebGlRenderingContext::ARRAY_BUFFER, Some(&vertex_buffer));

    // Note that `Float32Array::view` is somewhat dangerous (hence the
    // `unsafe`!). This is creating a raw view into our module's
    // `WebAssembly.Memory` buffer, but if we allocate more pages for ourself
    // (aka do a memory allocation in Rust) it'll cause the buffer to change,
    // causing the `Float32Array` to be invalid.
    //
    // As a result, after `Float32Array::view` we have to be very careful not to
    // do any memory allocations before it's dropped.
    unsafe {
        let vert_array = js_sys::Float32Array::view(&VERTICES);
        context.buffer_data_with_array_buffer_view(
            WebGlRenderingContext::ARRAY_BUFFER,
            &vert_array,
            WebGlRenderingContext::STATIC_DRAW,
        );
    }

    context.vertex_attrib_pointer_with_i32(
        position_location as u32,
        3,
        WebGlRenderingContext::FLOAT,
        false,
        0,
        0,
    );
    context.enable_vertex_attrib_array(position_location as u32);

    // Add uvs
    let uvs: [f32; 12] = [
        0.0, 1.0, // Bottom left
        1.0, 1.0, // Bottem right
        1.0, 0.0, // Top right
        0.0, 1.0, // Bottom left
        1.0, 0.0, // Top right
        0.0, 0.0, // Top left
    ];

    let uv_buffer = context
        .create_buffer()
        .ok_or("failed to create uv buffer")?;
    context.bind_buffer(WebGlRenderingContext::ARRAY_BUFFER, Some(&uv_buffer));

    // Note that `Float32Array::view` is somewhat dangerous (hence the
    // `unsafe`!). This is creating a raw view into our module's
    // `WebAssembly.Memory` buffer, but if we allocate more pages for ourself
    // (aka do a memory allocation in Rust) it'll cause the buffer to change,
    // causing the `Float32Array` to be invalid.
    //
    // As a result, after `Float32Array::view` we have to be very careful not to
    // do any memory allocations before it's dropped.
    unsafe {
        let uv_array = js_sys::Float32Array::view(&uvs);
        context.buffer_data_with_array_buffer_view(
            WebGlRenderingContext::ARRAY_BUFFER,
            &uv_array,
            WebGlRenderingContext::STATIC_DRAW,
        );
    }

    context.vertex_attrib_pointer_with_i32(
        texcoord_location as u32,
        2,
        WebGlRenderingContext::FLOAT,
        false,
        0,
        0,
    );
    context.enable_vertex_attrib_array(texcoord_location as u32);

    // Create a texture
    let texture = context.create_texture();
    context.bind_texture(WebGlRenderingContext::TEXTURE_2D, texture.as_ref());
    unsafe {
        context
            .tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
                //context.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
                WebGlRenderingContext::TEXTURE_2D,
                0,
                WebGlRenderingContext::RGBA as i32,
                WIDTH,
                HEIGHT,
                0,
                WebGlRenderingContext::RGBA,
                WebGlRenderingContext::UNSIGNED_BYTE,
                Some(&PIXEL_DATA),
            )
            .expect("should create GPU memory OK");
    }
    context.generate_mipmap(WebGlRenderingContext::TEXTURE_2D);
    context.tex_parameteri(
        WebGlRenderingContext::TEXTURE_2D,
        WebGlRenderingContext::TEXTURE_WRAP_S,
        WebGlRenderingContext::CLAMP_TO_EDGE as i32,
    );
    context.tex_parameteri(
        WebGlRenderingContext::TEXTURE_2D,
        WebGlRenderingContext::TEXTURE_WRAP_T,
        WebGlRenderingContext::CLAMP_TO_EDGE as i32,
    );
    context.tex_parameteri(
        WebGlRenderingContext::TEXTURE_2D,
        WebGlRenderingContext::TEXTURE_MAG_FILTER,
        WebGlRenderingContext::LINEAR as i32,
    );

    context.uniform1i(Some(texture_location.unwrap().as_ref()), 0);
    // draw()
    context.clear_color(0.0, 0.0, 0.0, 1.0);
    context.clear(WebGlRenderingContext::COLOR_BUFFER_BIT);
    context.draw_arrays(
        WebGlRenderingContext::TRIANGLES,
        0,
        (VERTICES.len() / 3) as i32,
    );

    input_data_update_loop(context, texture.unwrap());

    // Fin
    Ok(())
}

pub fn input_data_update_loop(gl: WebGlRenderingContext, texture: web_sys::WebGlTexture) {
    let f = Rc::new(RefCell::new(None));
    let g = f.clone();

    {
        *g.borrow_mut() = Some(Closure::wrap(Box::new(move || {
            gl.bind_texture(WebGlRenderingContext::TEXTURE_2D, Some(&texture));
            unsafe {
                if PIXEL_DATA_UPDATED == true {
                    gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_u8_array(
                        WebGlRenderingContext::TEXTURE_2D,
                        0,
                        0,
                        0,
                        WIDTH,
                        HEIGHT,
                        WebGlRenderingContext::RGBA,
                        WebGlRenderingContext::UNSIGNED_BYTE,
                        Some(&PIXEL_DATA),
                    )
                    .expect("should update GPU memory OK");
                    PIXEL_DATA_UPDATED = false;
                }
            }

            gl.clear_color(0.0, 0.0, 0.0, 1.0);
            gl.clear(WebGlRenderingContext::COLOR_BUFFER_BIT);
            gl.draw_arrays(
                WebGlRenderingContext::TRIANGLES,
                0,
                (VERTICES.len() / 3) as i32,
            );
            //update_texture_and_draw(gl, texture, texture_location);
            request_animation_frame(f.borrow().as_ref().unwrap());
        }) as Box<dyn FnMut()>));
    }

    request_animation_frame(g.borrow().as_ref().unwrap());
}

pub fn compile_shader(
    context: &WebGlRenderingContext,
    shader_type: u32,
    source: &str,
) -> Result<WebGlShader, String> {
    let shader = context
        .create_shader(shader_type)
        .ok_or_else(|| String::from("Unable to create shader object"))?;
    context.shader_source(&shader, source);
    context.compile_shader(&shader);

    if context
        .get_shader_parameter(&shader, WebGlRenderingContext::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(context
            .get_shader_info_log(&shader)
            .unwrap_or_else(|| String::from("Unknown error creating shader")))
    }
}

pub fn link_program(
    context: &WebGlRenderingContext,
    vert_shader: &WebGlShader,
    frag_shader: &WebGlShader,
) -> Result<WebGlProgram, String> {
    let program = context
        .create_program()
        .ok_or_else(|| String::from("Unable to create shader object"))?;

    context.attach_shader(&program, vert_shader);
    context.attach_shader(&program, frag_shader);
    context.link_program(&program);

    if context
        .get_program_parameter(&program, WebGlRenderingContext::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(context
            .get_program_info_log(&program)
            .unwrap_or_else(|| String::from("Unknown error creating program object")))
    }
}

#[wasm_bindgen]
pub fn copy(data: &ImageData) -> Result<(), JsValue> {
    unsafe {
        // TODO use mutex
        if PIXEL_DATA_UPDATED == false && PIXEL_DATA_UPDATING == false {
            PIXEL_DATA_UPDATING = true;
            for i in 0..BUFFER_SIZE {
                PIXEL_DATA[i] = data.data()[i];
            }
            PIXEL_DATA_UPDATING = false;
            PIXEL_DATA_UPDATED = true;
        }
    }

    Ok(())
}
