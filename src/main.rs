mod display;

use dbus::{
    arg::{Array, Dict, RefArg, Variant},
    blocking::Connection,
    message::MatchRule,
    Path,
};
use nix::sys::{
    memfd::MemFdCreateFlag,
    mman::{MapFlags, ProtFlags},
};
use pipewire::{
    self as pw, properties,
    spa::Direction,
    stream::{Stream, StreamFlags},
    Context, MainLoop, Properties,
};
use pipewire_sys as pw_sys;
use pw::spa::{format::{MediaSubtype, MediaType}, pod::Pod};
use std::{
    any::Any,
    collections::HashMap,
    ffi::CStr,
    fs::File,
    io::{Read, Seek},
    num::NonZeroUsize,
    os::{
        fd::{AsFd, AsRawFd, FromRawFd, RawFd},
        unix::fs::MetadataExt,
    },
    sync::{Mutex, Once, OnceLock, mpsc},
    time::Duration,
};

static sess_handle: OnceLock<Path> = OnceLock::new();
static streams_stat: OnceLock<Vec<(u32, HashMap<String, Variant<Box<dyn RefArg>>>)>> =
    OnceLock::new();

fn main() {
    let (frame_tx, frame_rx) = mpsc::channel::<(u32, u32)>();
    std::thread::spawn(|| run(frame_tx).unwrap());
    display::dmain(frame_rx);
}

fn run(frame_tx: mpsc::Sender<(u32, u32)>) -> Result<(), Box<dyn std::error::Error>> {
    // First open up a connection to the session bus.
    let conn = Connection::new_session()?;

    let mr = MatchRule::new_signal("org.freedesktop.portal.Request", "Response");

    // Second, create a wrapper struct around the connection that makes it easy
    // to send method calls to a specific destination and path.
    let proxy = conn.with_proxy(
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        Duration::from_millis(5000),
    );

    conn.add_match(
        mr.clone(),
        |(idk, x): (u32, HashMap<String, Variant<String>>), b, c| {
            sess_handle
                .set(Path::new(x["session_handle"].0.clone()).unwrap())
                .unwrap();
            false
        },
    )?;

    // Now make the method call. The ListNames method call takes zero input parameters and
    // one output parameter which is an array of strings.
    // Therefore the input is a zero tuple "()", and the output is a single tuple "(names,)".
    let (handle,): (Path,) = proxy.method_call(
        "org.freedesktop.portal.ScreenCast",
        "CreateSession",
        (
            dbus::arg::Dict::<String, dbus::arg::Variant<String>, _>::new([
                (
                    "handle_token".into(),
                    dbus::arg::Variant("dajdo299r2rjkOIJOJOI".into()),
                ),
                (
                    "session_handle_token".into(),
                    dbus::arg::Variant("oieoi19219391dmm".into()),
                ),
            ]),
        ),
    )?;
    while conn.process(Duration::from_millis(1000))? {}

    // Let's print all the names to stdout.
    println!("{}", handle);

    conn.add_match(
        mr.clone(),
        |(idk, x): (u32, HashMap<String, Variant<String>>), b, c| {
            dbg!(&x);
            false
        },
    )?;

    let (handle,): (Path,) = proxy.method_call(
        "org.freedesktop.portal.ScreenCast",
        "SelectSources",
        (
            sess_handle.get().unwrap().clone(),
            dbus::arg::Dict::<String, dbus::arg::Variant<String>, _>::new([(
                "handle_token".into(),
                dbus::arg::Variant("KJASDNnd218jOIJAOIj".into()),
            )]),
        ),
    )?;
    while conn.process(Duration::from_millis(1000))? {}

    println!("{}", handle);

    conn.add_match(
        mr,
        |(idk, mut x): (
            u32,
            HashMap<String, Variant<Vec<(u32, HashMap<String, Variant<Box<dyn RefArg>>>)>>>,
        ),
         b,
         c| {
            dbg!(&x);
            streams_stat.set(x.remove("streams").unwrap().0).unwrap();
            false
        },
    )?;
    let (handle,): (Path,)/*(streams,): (Vec<(u32, HashMap<String, Variant<Box<dyn RefArg>>>)>,)*/ = proxy.method_call(
        "org.freedesktop.portal.ScreenCast",
        "Start",
        (
            sess_handle.get().unwrap().clone(),
            "".to_string(),
            dbus::arg::Dict::<String, dbus::arg::Variant<String>, _>::new([
                (
                    "handle_token".into(),
                    dbus::arg::Variant("ISISDdajsjjas".into()),
                ),
            ]),
        ),
    )?;
    while { conn.process(Duration::from_millis(5000))? } {}

    println!("{}", handle);

    println!("{:#?}", streams_stat);
    let id = streams_stat.get().unwrap()[0].0;

    let mainloop = MainLoop::new()?;
    let context = Context::new(&mainloop)?;
    let core = context.connect(None)?;
    let registry = core.get_registry()?;

    let stream = Stream::new(
        &core,
        "jeff",
        properties! {
            *pipewire::keys::MEDIA_TYPE => "Video",
            *pipewire::keys::MEDIA_CATEGORY => "Capture",
            *pipewire::keys::MEDIA_ROLE => "Camera",
        },
    )?;
    let _listener = stream
        .add_local_listener::<()>()
        .state_changed(|old, new| {
            println!("State changed: {:?} -> {:?}", old, new);
        })
        .param_changed(|sr, id, user_data, param| {
            dbg!(user_data, id,);

            let Some(param) = param else {
                return;
            };

            let (media_type, media_subtype) =
                match pw::spa::param::format_utils::parse_format(param) {
                    Ok(v) => v,
                    Err(_) => return,
                };

            dbg!(media_type, media_subtype);
            if media_type != MediaType::Video || media_subtype != MediaSubtype::Raw {
                return;
            }

            // prepare to render video of this size
        })
        .process(move |stream, _| {
            match stream.dequeue_buffer() {
                None => println!("out of buffers"),
                Some(mut buffer) => {
                    let datas = buffer.datas_mut();
                    if datas.is_empty() {
                        return;
                    }

                    // copy frame data to screen
                    let data = &mut datas[0];
                    let d = data.as_raw();
                    println!("got a frame of size {}", data.chunk().size());

                    let mut b = display::dbuf.lock().unwrap();

                    let mut f = unsafe { File::from_raw_fd(d.fd as i32) };

                    b.resize(data.chunk().size() as usize, 0);
                    f.seek(std::io::SeekFrom::Start(0)).unwrap();
                    f.read(&mut b).unwrap();

                    std::mem::forget(f);
                    frame_tx.send((1920, 1200)).unwrap();
                }
            }
        })
        .register()?;

    let obj = pw::spa::pod::object!(
        pw::spa::utils::SpaTypes::ObjectParamFormat,
        pw::spa::param::ParamType::EnumFormat,
        pw::spa::pod::property! {
            pw::spa::format::FormatProperties::MediaType,
            Id,
            pw::spa::format::MediaType::Video
        },
        pw::spa::pod::property!(
            pw::spa::format::FormatProperties::MediaSubtype,
            Id,
            pw::spa::format::MediaSubtype::Raw
        ),
    );


    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(obj),
    )
    .unwrap()
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).unwrap()];

    stream.connect(
        Direction::Input,
        Some(id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::ALLOC_BUFFERS,
        &mut params,
    )?;

    dbg!("stream conn'd");

    mainloop.run();

    Ok(())
}
