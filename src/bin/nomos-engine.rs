use std::{env, path::PathBuf};

use nomos_engine::Runtime;
use signal_nomos::{Request, encode_reply};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

type AnyError = Box<dyn std::error::Error + Send + Sync>;

// [to-be-reviewed-by-psyche] Wire revision 0 admits at most 16 MiB per
// length-prefixed request.
const MAX_REQUEST_FRAME_BYTES: usize = 16 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), AnyError> {
    let mut arguments = env::args().skip(1);
    let Some("daemon") = arguments.next().as_deref() else {
        return Err("usage: nomos-engine daemon <socket> <nomos.sema> <admin-uid>".into());
    };
    let socket = PathBuf::from(arguments.next().ok_or("missing daemon socket")?);
    let database = PathBuf::from(arguments.next().ok_or("missing nomos.sema path")?);
    let admin_uid = arguments
        .next()
        .ok_or("missing configured admin UID")?
        .parse::<u32>()?;
    if arguments.next().is_some() {
        return Err("too many daemon arguments".into());
    }
    if let Some(parent) = socket.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let _ = std::fs::remove_file(&socket);
    let runtime = Runtime::open(&database, admin_uid)?;
    let listener = UnixListener::bind(&socket)?;
    println!("READY {}", socket.display());
    loop {
        let (stream, _) = listener.accept().await?;
        let runtime = runtime.clone();
        tokio::spawn(async move {
            let _ = serve(stream, runtime).await;
        });
    }
}

async fn serve(mut stream: UnixStream, runtime: Runtime) -> Result<(), AnyError> {
    let peer_uid = stream.peer_cred()?.uid();
    loop {
        let length = match stream.read_u32().await {
            Ok(length) => length as usize,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if length > MAX_REQUEST_FRAME_BYTES {
            return Err("request frame exceeds the configured maximum".into());
        }
        let mut payload = vec![0; length];
        stream.read_exact(&mut payload).await?;
        let request = rkyv::from_bytes::<Request, rkyv::rancor::Error>(&payload)?;
        let reply = runtime.request_as(peer_uid, request).await?;
        let encoded = encode_reply(&reply)?;
        stream.write_u32(u32::try_from(encoded.len())?).await?;
        stream.write_all(&encoded).await?;
    }
}
