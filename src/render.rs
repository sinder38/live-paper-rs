use glow::HasContext;

/// Test pattern to draw
#[derive(Clone, Copy)]
pub enum Pattern {
    Rainbow,
    Checkerboard,
}

/// Size of checkerboard square
const SQUARE_PX: f32 = 1.0;

/// A fullscreen triangle generated from gl_VertexID
const VERT_SRC: &str = "#version 300 es
void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2));
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}";

/// Black/white checkerboard keyed on the physical pixel coordinate
const CHECKER_SRC: &str = "#version 300 es
precision mediump float;
out vec4 color;
uniform float square;
void main() {
    vec2 cell = floor(gl_FragCoord.xy / square);
    float v = mod(cell.x + cell.y, 2.0);
    color = vec4(vec3(v), 1.0);
}";

/// Holds the compiled GL programs
pub struct Renderer {
    checker: glow::Program,
    vertex_arr: glow::VertexArray,
}

impl Renderer {
    pub fn new(gl: &glow::Context) -> Self {
        unsafe {
            let vertex_arr = gl.create_vertex_array().expect("create vertex array");
            let checker = compile(gl, VERT_SRC, CHECKER_SRC);
            Self {
                checker,
                vertex_arr,
            }
        }
    }

    pub fn draw(&self, gl: &glow::Context, pattern: Pattern, width: i32, height: i32, time: u32) {
        unsafe {
            gl.viewport(0, 0, width, height);

            // match is fine for a test
            match pattern {
                Pattern::Rainbow => {
                    let t = time as f32 * 0.001;
                    let wave = |phase: f32| 0.5 + 0.5 * (t + phase).sin();
                    gl.clear_color(wave(0.0), wave(2.094), wave(4.188), 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT);
                }
                Pattern::Checkerboard => {
                    gl.use_program(Some(self.checker));
                    let loc = gl.get_uniform_location(self.checker, "square");
                    gl.uniform_1_f32(loc.as_ref(), SQUARE_PX);
                    gl.bind_vertex_array(Some(self.vertex_arr));
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                }
            }
        }
    }
}

// Works
fn compile(gl: &glow::Context, vert_src: &str, frag_src: &str) -> glow::Program {
    unsafe {
        let program = gl.create_program().expect("create program");
        for (kind, src) in [
            (glow::VERTEX_SHADER, vert_src),
            (glow::FRAGMENT_SHADER, frag_src),
        ] {
            let shader = gl.create_shader(kind).expect("create shader");
            gl.shader_source(shader, src);
            gl.compile_shader(shader);
            assert!(
                gl.get_shader_compile_status(shader),
                "shader compile failed: {}",
                gl.get_shader_info_log(shader)
            );
            gl.attach_shader(program, shader);
            gl.delete_shader(shader);
        }
        gl.link_program(program);
        assert!(
            gl.get_program_link_status(program),
            "program link failed: {}",
            gl.get_program_info_log(program)
        );
        program
    }
}
