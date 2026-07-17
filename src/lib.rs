use std::{convert::Infallible, path::PathBuf};

use core_nomos::MacroPackage;
use kameo::{
    Actor,
    actor::{ActorRef, Spawn},
    message::{Context, Message},
};
use name_table::NameTable;
use signal_nomos::{Rejection, Reply, Request, TransformEvent};
use signal_sema_storage::{
    DocumentKey, DocumentKind, DocumentPayload, FrameMessage, NameTableBytes, NomosPackage,
    Reply as SemaReply, Request as SemaRequest, SlotIdentifier, SubscriptionIdentifier, Wire,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
    sync::broadcast,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("engine: {0}")]
    Engine(String),
    #[error("actor: {0}")]
    Actor(String),
}
type Result<T> = std::result::Result<T, Error>;

pub struct SemaPlane {
    socket: PathBuf,
    writes: u64,
}
impl SemaPlane {
    async fn read_frame(&self, stream: &mut UnixStream) -> Result<Vec<u8>> {
        let length = stream.read_u32().await? as usize;
        let mut frame = Vec::with_capacity(length + 4);
        frame.extend_from_slice(&(length as u32).to_be_bytes());
        frame.resize(length + 4, 0);
        stream.read_exact(&mut frame[4..]).await?;
        Ok(frame)
    }

    async fn exchange(&self, request: &SemaRequest) -> Result<SemaReply> {
        let mut stream = UnixStream::connect(&self.socket).await?;
        stream
            .write_all(
                &Wire::frame_current_handshake_request()
                    .map_err(|error| Error::Engine(error.to_string()))?,
            )
            .await?;
        let handshake = Wire::decode_frame(&self.read_frame(&mut stream).await?)
            .map_err(|error| Error::Engine(error.to_string()))?;
        if !handshake.is_accepted_handshake() {
            return Err(Error::Engine("Sema rejected frame protocol".into()));
        }
        let payload =
            Wire::encode_request(request).map_err(|error| Error::Engine(error.to_string()))?;
        stream
            .write_all(
                &Wire::frame_request(payload, self.writes)
                    .map_err(|error| Error::Engine(error.to_string()))?,
            )
            .await?;
        let FrameMessage::Reply { payload, .. } =
            Wire::decode_frame(&self.read_frame(&mut stream).await?)
                .map_err(|error| Error::Engine(error.to_string()))?
        else {
            return Err(Error::Engine("Sema returned a non-reply frame".into()));
        };
        rkyv::from_bytes::<SemaReply, rkyv::rancor::Error>(&payload)
            .map_err(|error| Error::Engine(error.to_string()))
    }
}
impl Actor for SemaPlane {
    type Args = Self;
    type Error = Infallible;
    async fn on_start(
        actor: Self::Args,
        _: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }
}
pub struct Store(pub SemaRequest);
impl Message<Store> for SemaPlane {
    type Reply = Result<SemaReply>;
    async fn handle(&mut self, message: Store, _: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.writes += 1;
        self.exchange(&message.0).await
    }
}

pub struct NexusPlane {
    sema: ActorRef<SemaPlane>,
    events: broadcast::Sender<TransformEvent>,
    applied: u64,
}
impl Actor for NexusPlane {
    type Args = Self;
    type Error = Infallible;
    async fn on_start(
        actor: Self::Args,
        _: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }
}
pub struct Dispatch(pub Request);
impl Message<Dispatch> for NexusPlane {
    type Reply = Result<Reply>;
    async fn handle(
        &mut self,
        message: Dispatch,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        match message.0 {
            Request::Transform {
                scope,
                schema,
                output_slot,
            } => {
                self.applied += 1;
                let fetched = self
                    .sema
                    .ask(Store(SemaRequest::HashFetch { hash: schema }))
                    .send()
                    .await
                    .map_err(|error| Error::Actor(error.to_string()))?;
                let SemaReply::Document(Some(document)) = fetched else {
                    return Ok(Reply::Rejected(Rejection::SchemaNotFound));
                };
                let DocumentPayload::TypeSchema {
                    schema: core,
                    names,
                } = document.payload
                else {
                    return Ok(Reply::Rejected(Rejection::WrongDocumentKind));
                };
                let names = NameTable::from_archive_bytes(&names.0)
                    .map_err(|error| Error::Engine(error.to_string()))?;
                // LEAN `engine-selects-enriched`: `Request::Transform` carries no
                // package selector, so the engine applies the enriched package
                // unconditionally — declarations plus generation classes
                // A/B/C-stub/D in the golden's document order. The plain and wire
                // fixtures remain available on `MacroPackage` for a future request
                // surface that distinguishes them. Trigger to revisit: adding a
                // package selector to the Nomos transform request.
                let lowering = MacroPackage::enriched_fixture()
                    .apply_enriched(&core, &names)
                    .map_err(|error| Error::Engine(error.to_string()))?;
                let logos_names = NameTableBytes(
                    lowering
                        .names
                        .to_archive_bytes()
                        .map_err(|error| Error::Engine(error.to_string()))?
                        .to_vec(),
                );
                let _ = self
                    .sema
                    .ask(Store(SemaRequest::Store {
                        key: DocumentKey {
                            scope,
                            kind: DocumentKind::Nomos,
                            slot: SlotIdentifier(0),
                        },
                        payload: DocumentPayload::Nomos(NomosPackage::WireFixture),
                    }))
                    .send()
                    .await
                    .map_err(|error| Error::Actor(error.to_string()))?;
                let stored = self
                    .sema
                    .ask(Store(SemaRequest::Store {
                        key: DocumentKey {
                            scope,
                            kind: DocumentKind::Logos,
                            slot: output_slot,
                        },
                        payload: DocumentPayload::Logos {
                            items: lowering.items,
                            names: logos_names,
                        },
                    }))
                    .send()
                    .await
                    .map_err(|error| Error::Actor(error.to_string()))?;
                Ok(match stored {
                    SemaReply::Stored(summary) => {
                        let _ = self.events.send(TransformEvent {
                            schema,
                            logos: summary.clone(),
                        });
                        Reply::Transformed(summary)
                    }
                    _ => Reply::Rejected(Rejection::StorageFailed),
                })
            }
            Request::List { scope } => {
                let reply = self
                    .sema
                    .ask(Store(SemaRequest::List {
                        scope,
                        kind: Some(DocumentKind::Nomos),
                    }))
                    .send()
                    .await
                    .map_err(|error| Error::Actor(error.to_string()))?;
                Ok(match reply {
                    SemaReply::Listed(values) => Reply::Listed(values),
                    _ => Reply::Rejected(Rejection::StorageFailed),
                })
            }
            Request::Subscribe { scope } => {
                let reply = self
                    .sema
                    .ask(Store(SemaRequest::List {
                        scope,
                        kind: Some(DocumentKind::Logos),
                    }))
                    .send()
                    .await
                    .map_err(|error| Error::Actor(error.to_string()))?;
                Ok(match reply {
                    SemaReply::Listed(initial) => Reply::Subscribed {
                        identifier: SubscriptionIdentifier(0),
                        initial,
                    },
                    _ => Reply::Rejected(Rejection::StorageFailed),
                })
            }
        }
    }
}

pub struct SignalPlane {
    nexus: ActorRef<NexusPlane>,
    admitted: u64,
}
impl Actor for SignalPlane {
    type Args = Self;
    type Error = Infallible;
    async fn on_start(
        actor: Self::Args,
        _: ActorRef<Self>,
    ) -> std::result::Result<Self, Self::Error> {
        Ok(actor)
    }
}
impl Message<Dispatch> for SignalPlane {
    type Reply = Result<Reply>;
    async fn handle(
        &mut self,
        message: Dispatch,
        _: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.admitted += 1;
        self.nexus
            .ask(message)
            .send()
            .await
            .map_err(|error| Error::Actor(error.to_string()))
    }
}

#[derive(Clone)]
pub struct Runtime {
    signal: ActorRef<SignalPlane>,
    events: broadcast::Sender<TransformEvent>,
}
impl Runtime {
    pub fn new(socket: PathBuf) -> Self {
        let sema = SemaPlane::spawn(SemaPlane { socket, writes: 0 });
        let (events, _) = broadcast::channel(64);
        let nexus = NexusPlane::spawn(NexusPlane {
            sema,
            events: events.clone(),
            applied: 0,
        });
        Self {
            signal: SignalPlane::spawn(SignalPlane { nexus, admitted: 0 }),
            events,
        }
    }
    pub async fn request(&self, request: Request) -> Result<Reply> {
        self.signal
            .ask(Dispatch(request))
            .send()
            .await
            .map_err(|error| Error::Actor(error.to_string()))
    }
    pub fn subscribe(&self) -> broadcast::Receiver<TransformEvent> {
        self.events.subscribe()
    }
}
