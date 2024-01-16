use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Mutex, mpsc};
use winit::event::{Event, KeyEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::WindowBuilder;

pub static dbuf: Mutex<Vec<u8>> = Mutex::new(vec![]);

pub fn dmain(frame_rx: mpsc::Receiver<(u32, u32)>) {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new().build(&event_loop).unwrap());

    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    event_loop
        .run(move |event, elwt| {
            elwt.set_control_flow(ControlFlow::Wait);

            if frame_rx.try_recv().is_ok() {
                window.request_redraw();
            }

            match event {
                Event::WindowEvent {
                    window_id,
                    event: WindowEvent::RedrawRequested,
                } if window_id == window.id() => {
                    if let (Some(width), Some(height)) = {
                        let size = window.inner_size();
                        (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
                    } {
                        let (width, height) = (1920.try_into().unwrap(), 1200.try_into().unwrap());
                        surface.resize(width, height).unwrap();

                        let bf = dbuf.lock().unwrap();

                        let mut buffer = surface.buffer_mut().unwrap();
                        if bf.len() != 0 {
                            let bufptr = &mut *buffer as *mut [u32] as *mut u32 as *mut u8;
                            let buffer = unsafe { std::slice::from_raw_parts_mut(bufptr, buffer.len() * 4) };
                            buffer.copy_from_slice(&*bf);
                        }

                        buffer.present().unwrap();
                    }
                }
                Event::WindowEvent {
                    event:
                        WindowEvent::CloseRequested
                        | WindowEvent::KeyboardInput {
                            event:
                                KeyEvent {
                                    logical_key: Key::Named(NamedKey::Escape),
                                    ..
                                },
                            ..
                        },
                    window_id,
                } if window_id == window.id() => {
                    elwt.exit();
                }
                _ => {}
            }
        })
        .unwrap();
}