use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use nomos_engine::Runtime;
use rkyv::{Archive, Deserialize};
use signal_nomos::{Reply as NomosReply, Request as NomosRequest};
use signal_schema::{Reply as SchemaReply, Request as SchemaRequest, encode_reply, encode_request};
use signal_sema_storage::{
    DocumentKind, FixtureScope, FrameMessage, Reply as SemaReply, Request as SemaRequest, Wire,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

fn temporary_socket_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time precedes Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("nomos-engine-{nonce}.sock"))
}

async fn read_frame(
    stream: &mut tokio::net::UnixStream,
) -> std::result::Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let length = stream.read_u32().await? as usize;
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&(length as u32).to_be_bytes());
    frame.resize(length + 4, 0);
    stream.read_exact(&mut frame[4..]).await?;
    Ok(frame)
}

fn decode<T>(bytes: &[u8]) -> std::result::Result<T, rkyv::rancor::Error>
where
    T: Archive,
    T::Archived: for<'a> rkyv::bytecheck::CheckBytes<rkyv::api::high::HighValidator<'a, rkyv::rancor::Error>>
        + Deserialize<T, rkyv::api::high::HighDeserializer<rkyv::rancor::Error>>,
{
    rkyv::from_bytes(bytes)
}

#[test]
fn schema_subscription_request_and_reply_round_trip_with_encoded_storage_types() {
    let request = SchemaRequest::Subscribe {
        scope: FixtureScope(9),
        kind: Some(DocumentKind::TypeSchema),
    };
    let request_bytes = encode_request(&request).expect("schema request encodes");
    assert_eq!(
        decode::<SchemaRequest>(&request_bytes).expect("schema request decodes"),
        request
    );

    let reply = SchemaReply::Rejected(signal_schema::Rejection::StorageFailed);
    let reply_bytes = encode_reply(&reply).expect("schema reply encodes");
    assert_eq!(
        decode::<SchemaReply>(&reply_bytes).expect("schema reply decodes"),
        reply
    );
}

#[tokio::test]
async fn runtime_list_request_uses_the_updated_sema_request_reply_contract() {
    let socket_path = temporary_socket_path();
    let listener = UnixListener::bind(&socket_path).expect("bind test SEMA socket");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await?;
        let FrameMessage::HandshakeRequest(peer) =
            Wire::decode_frame(&read_frame(&mut stream).await?)?
        else {
            return Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                "expected handshake request".into(),
            );
        };
        stream
            .write_all(&Wire::frame_handshake_reply(Wire::handshake_reply(peer))?)
            .await?;

        let FrameMessage::Request { exchange, payload } =
            Wire::decode_frame(&read_frame(&mut stream).await?)?
        else {
            return Err("expected SEMA request frame".into());
        };
        let request: SemaRequest = decode(&payload)?;
        assert_eq!(
            request,
            SemaRequest::List {
                scope: FixtureScope(4),
                kind: Some(DocumentKind::Nomos),
            }
        );
        let payload =
            rkyv::to_bytes::<rkyv::rancor::Error>(&SemaReply::Listed(Vec::new()))?.to_vec();
        stream
            .write_all(&Wire::frame_reply(exchange, payload)?)
            .await?;
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });

    let runtime = Runtime::new(socket_path.clone());
    let reply = runtime
        .request(NomosRequest::List {
            scope: FixtureScope(4),
        })
        .await
        .expect("runtime request succeeds");
    assert_eq!(reply, NomosReply::Listed(Vec::new()));
    server
        .await
        .expect("SEMA task joins")
        .expect("SEMA task succeeds");
    std::fs::remove_file(socket_path).expect("remove test socket");
}
