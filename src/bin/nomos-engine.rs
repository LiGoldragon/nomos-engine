use std::{env, path::PathBuf, time::Duration};

use nomos_engine::Runtime;
use signal_nomos::{Reply, Request, encode_reply, encode_request};
use signal_sema_storage::{ContentHash, DocumentKind, FixtureScope, SlotIdentifier};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut arguments = env::args().skip(1);
    match arguments.next().as_deref() {
        Some("daemon") => {
            let socket = PathBuf::from(
                arguments
                    .next()
                    .unwrap_or_else(|| "/tmp/new-language-engine/nomos.sock".into()),
            );
            let sema = PathBuf::from(
                arguments
                    .next()
                    .unwrap_or_else(|| "/tmp/new-language-engine/sema.sock".into()),
            );
            let schema = PathBuf::from(
                arguments
                    .next()
                    .unwrap_or_else(|| "/tmp/new-language-engine/schema.sock".into()),
            );
            if let Some(parent) = socket.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _ = std::fs::remove_file(&socket);
            let listener = UnixListener::bind(socket)?;
            let runtime = Runtime::new(sema);
            let relay_runtime = runtime.clone();
            tokio::spawn(async move {
                relay_schema(schema, relay_runtime).await;
            });
            loop {
                let (stream, _) = listener.accept().await?;
                let runtime = runtime.clone();
                tokio::spawn(async move {
                    let _ = serve(stream, runtime).await;
                });
            }
        }
        Some("transform") => {
            let socket = PathBuf::from(arguments.next().ok_or("socket")?);
            let hash = ContentHash(parse_hash(&arguments.next().ok_or("schema hash")?)?);
            println!(
                "{:?}",
                client(
                    &socket,
                    &Request::Transform {
                        scope: FixtureScope(1),
                        schema: hash,
                        output_slot: SlotIdentifier(1),
                    },
                )
                .await?
            );
            Ok(())
        }
        Some("subscribe") => {
            let socket = PathBuf::from(arguments.next().ok_or("socket")?);
            subscribe(&socket).await
        }
        _ => Err("usage: nomos-engine daemon [socket] [sema-socket] [schema-socket] | transform <socket> <hash> | subscribe <socket>".into()),
    }
}

async fn relay_schema(schema: PathBuf, runtime: Runtime) {
    loop {
        if relay_schema_connection(&schema, &runtime).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn relay_schema_connection(
    schema: &PathBuf,
    runtime: &Runtime,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(schema).await?;
    let request = signal_schema::Request::Subscribe {
        scope: FixtureScope(1),
        kind: Some(DocumentKind::TypeSchema),
    };
    let bytes = signal_schema::encode_request(&request).map_err(|error| error.to_string())?;
    write_bytes(&mut stream, &bytes).await?;
    let _: signal_schema::Reply = read_value(&mut stream).await?;
    loop {
        if let signal_schema::Reply::Event(event) = read_value(&mut stream).await? {
            let _ = runtime
                .request(Request::Transform {
                    scope: event.document.key.scope,
                    schema: event.document.hash,
                    output_slot: event.document.key.slot,
                })
                .await?;
        }
    }
}

async fn serve(
    mut stream: UnixStream,
    runtime: Runtime,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let request: Request = read_value(&mut stream).await?;
    let subscribed = matches!(request, Request::Subscribe { .. });
    let mut events = runtime.subscribe();
    write_reply(&mut stream, &runtime.request(request).await?).await?;
    if subscribed {
        while let Ok(event) = events.recv().await {
            write_reply(&mut stream, &Reply::Event(event)).await?;
        }
    }
    Ok(())
}

async fn subscribe(socket: &PathBuf) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(socket).await?;
    let bytes = encode_request(&Request::Subscribe {
        scope: FixtureScope(1),
    })
    .map_err(|error| error.to_string())?;
    write_bytes(&mut stream, &bytes).await?;
    loop {
        println!("{:?}", read_value::<Reply>(&mut stream).await?);
    }
}

async fn client(
    path: &PathBuf,
    request: &Request,
) -> Result<Reply, Box<dyn std::error::Error + Send + Sync>> {
    let mut stream = UnixStream::connect(path).await?;
    let bytes = encode_request(request).map_err(|error| error.to_string())?;
    write_bytes(&mut stream, &bytes).await?;
    read_value(&mut stream).await
}

async fn write_reply(
    stream: &mut UnixStream,
    reply: &Reply,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = encode_reply(reply).map_err(|error| error.to_string())?;
    write_bytes(stream, &bytes).await
}
async fn write_bytes(
    stream: &mut UnixStream,
    bytes: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    stream.write_u32_le(bytes.len() as u32).await?;
    stream.write_all(bytes).await?;
    Ok(())
}
async fn read_value<T>(
    stream: &mut UnixStream,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>>
where
    T: rkyv::Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<
            rkyv::rancor::Strategy<
                rkyv::validation::Validator<
                    rkyv::validation::archive::ArchiveValidator<'a>,
                    rkyv::validation::shared::SharedValidator,
                >,
                rkyv::rancor::Error,
            >,
        > + rkyv::Deserialize<T, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    let length = stream.read_u32_le().await? as usize;
    let mut bytes = vec![0; length];
    stream.read_exact(&mut bytes).await?;
    Ok(rkyv::from_bytes::<T, rkyv::rancor::Error>(&bytes)?)
}
fn parse_hash(value: &str) -> Result<[u8; 32], Box<dyn std::error::Error + Send + Sync>> {
    if value.len() != 64 {
        return Err("hash must have 64 hexadecimal digits".into());
    }
    let mut output = [0; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        output[index] = u8::from_str_radix(std::str::from_utf8(pair)?, 16)?;
    }
    Ok(output)
}
