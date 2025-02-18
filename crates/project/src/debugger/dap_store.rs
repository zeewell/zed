use super::{
    breakpoint_store::BreakpointStore,
    // Will need to uncomment this once we implement rpc message handler again
    // dap_command::{
    //     ContinueCommand, DapCommand, DisconnectCommand, NextCommand, PauseCommand, RestartCommand,
    //     RestartStackFrameCommand, StepBackCommand, StepCommand, StepInCommand, StepOutCommand,
    //     TerminateCommand, TerminateThreadsCommand, VariablesCommand,
    // },
    dap_command::DapCommand,
    session::{self, Session},
};
use crate::{
    debugger, project_settings::ProjectSettings, DebugAdapterClientState, ProjectEnvironment,
    ProjectPath,
};
use anyhow::{anyhow, bail, Context as _, Result};
use async_trait::async_trait;
use collections::HashMap;
use dap::{
    adapters::{DapDelegate, DapStatus, DebugAdapter, DebugAdapterBinary, DebugAdapterName},
    client::{DebugAdapterClient, SessionId},
    messages::{Message, Response},
    requests::{
        Attach, Completions, Evaluate, Initialize, Launch, Request as _, RunInTerminal,
        SetBreakpoints, SetExpression, SetVariable, StartDebugging,
    },
    AttachRequestArguments, Capabilities, CompletionItem, CompletionsArguments, ErrorResponse,
    EvaluateArguments, EvaluateArgumentsContext, EvaluateResponse, InitializeRequestArguments,
    InitializeRequestArgumentsPathFormat, LaunchRequestArguments, SetBreakpointsArguments,
    SetExpressionArguments, SetVariableArguments, Source, SourceBreakpoint,
    StartDebuggingRequestArguments, StartDebuggingRequestArgumentsRequest,
};
use dap_adapters::build_adapter;
use fs::Fs;
use futures::future::Shared;
use gpui::{App, AppContext, AsyncApp, Context, Entity, EventEmitter, SharedString, Task};
use http_client::HttpClient;
use language::{BinaryStatus, BufferSnapshot, LanguageRegistry, LanguageToolchainStore};
use lsp::LanguageServerName;
use node_runtime::NodeRuntime;
use rpc::{
    proto::{self, UpdateDebugAdapter, UpdateThreadStatus},
    AnyProtoClient, TypedEnvelope,
};
use serde_json::Value;
use settings::{Settings as _, WorktreeId};
use smol::lock::Mutex;
use std::{
    borrow::Borrow,
    collections::{BTreeMap, HashSet},
    ffi::OsStr,
    hash::Hash,
    path::{Path, PathBuf},
    sync::{atomic::Ordering::SeqCst, Arc},
};
use std::{collections::VecDeque, sync::atomic::AtomicU32};
use task::{AttachConfig, DebugAdapterConfig, DebugRequestType};
use util::{merge_json_value_into, ResultExt as _};
use worktree::Worktree;

pub enum DapStoreEvent {
    DebugClientStarted(SessionId),
    DebugClientShutdown(SessionId),
    DebugClientEvent {
        session_id: SessionId,
        message: Message,
    },
    Notification(String),
    ActiveDebugLineChanged,
    RemoteHasInitialized,
    UpdateDebugAdapter(UpdateDebugAdapter),
    UpdateThreadStatus(UpdateThreadStatus),
}

#[allow(clippy::large_enum_variant)]
pub enum DapStoreMode {
    Local(LocalDapStore),   // ssh host and collab host
    Remote(RemoteDapStore), // collab guest
}

pub struct LocalDapStore {
    fs: Arc<dyn Fs>,
    node_runtime: NodeRuntime,
    next_session_id: AtomicU32,
    http_client: Arc<dyn HttpClient>,
    environment: Entity<ProjectEnvironment>,
    language_registry: Arc<LanguageRegistry>,
    toolchain_store: Arc<dyn LanguageToolchainStore>,
}

impl LocalDapStore {
    fn next_session_id(&self) -> SessionId {
        SessionId(self.next_session_id.fetch_add(1, SeqCst))
    }
    pub fn respond_to_start_debugging(
        &mut self,
        session: &Entity<Session>,
        seq: u64,
        args: Option<StartDebuggingRequestArguments>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let config = session.read(cx).configuration();

        let request_args = args.unwrap_or_else(|| StartDebuggingRequestArguments {
            configuration: config.initialize_args.clone().unwrap_or_default(),
            request: match config.request {
                DebugRequestType::Launch => StartDebuggingRequestArgumentsRequest::Launch,
                DebugRequestType::Attach(_) => StartDebuggingRequestArgumentsRequest::Attach,
            },
        });

        // Merge the new configuration over the existing configuration
        let mut initialize_args = config.initialize_args.clone().unwrap_or_default();
        merge_json_value_into(request_args.configuration, &mut initialize_args);

        let new_config = DebugAdapterConfig {
            label: config.label.clone(),
            kind: config.kind.clone(),
            request: match &request_args.request {
                StartDebuggingRequestArgumentsRequest::Launch => DebugRequestType::Launch,
                StartDebuggingRequestArgumentsRequest::Attach => DebugRequestType::Attach(
                    if let DebugRequestType::Attach(attach_config) = &config.request {
                        attach_config.clone()
                    } else {
                        AttachConfig::default()
                    },
                ),
            },
            program: config.program.clone(),
            cwd: config.cwd.clone(),
            initialize_args: Some(initialize_args),
            supports_attach: true,
        };

        cx.spawn(|this, mut cx| async move {
            let (success, body) = {
                let reconnect_task = this.update(&mut cx, |store, cx| {
                    if !unimplemented!("client.adapter().supports_attach()")
                        && matches!(new_config.request, DebugRequestType::Attach(_))
                    {
                        Task::<Result<()>>::ready(Err(anyhow!(
                            "Debug adapter does not support `attach` request"
                        )))
                    } else {
                        unimplemented!(
                            "store.reconnect_client(client.binary().clone(), new_config, cx)"
                        );
                    }
                });

                match reconnect_task {
                    Ok(task) => match task.await {
                        Ok(_) => (true, None),
                        Err(error) => (
                            false,
                            Some(serde_json::to_value(ErrorResponse {
                                error: Some(dap::Message {
                                    id: seq,
                                    format: error.to_string(),
                                    variables: None,
                                    send_telemetry: None,
                                    show_user: None,
                                    url: None,
                                    url_label: None,
                                }),
                            })?),
                        ),
                    },
                    Err(error) => (
                        false,
                        Some(serde_json::to_value(ErrorResponse {
                            error: Some(dap::Message {
                                id: seq,
                                format: error.to_string(),
                                variables: None,
                                send_telemetry: None,
                                show_user: None,
                                url: None,
                                url_label: None,
                            }),
                        })?),
                    ),
                }
            };
            unimplemented!();
            Ok(())
            /*client
            .send_message(Message::Response(Response {
                seq,
                body,
                success,
                request_seq: seq,
                command: StartDebugging::COMMAND.to_string(),
            }))
            .await*/
        })
    }
}

pub struct RemoteDapStore {
    upstream_client: AnyProtoClient,
    upstream_project_id: u64,
    event_queue: Option<VecDeque<DapStoreEvent>>,
}

pub struct DapStore {
    mode: DapStoreMode,
    downstream_client: Option<(AnyProtoClient, u64)>,
    breakpoint_store: Entity<BreakpointStore>,
    active_debug_line: Option<(SessionId, ProjectPath, u32)>,
    sessions: BTreeMap<SessionId, Entity<Session>>,
}

impl EventEmitter<DapStoreEvent> for DapStore {}

impl DapStore {
    pub fn init(client: &AnyProtoClient) {
        client.add_entity_message_handler(Self::handle_remove_active_debug_line);
        client.add_entity_message_handler(Self::handle_shutdown_debug_client);
        client.add_entity_message_handler(Self::handle_set_active_debug_line);
        client.add_entity_message_handler(Self::handle_set_debug_client_capabilities);
        client.add_entity_message_handler(Self::handle_update_debug_adapter);
        client.add_entity_message_handler(Self::handle_update_thread_status);
        client.add_entity_message_handler(Self::handle_ignore_breakpoint_state);

        // todo(debugger): Reenable these after we finish handle_dap_command refactor
        // client.add_entity_request_handler(Self::handle_dap_command::<NextCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<StepInCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<StepOutCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<StepBackCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<ContinueCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<PauseCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<DisconnectCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<TerminateThreadsCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<TerminateCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<RestartCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<VariablesCommand>);
        // client.add_entity_request_handler(Self::handle_dap_command::<RestartStackFrameCommand>);
    }

    pub fn new_local(
        http_client: Arc<dyn HttpClient>,
        node_runtime: NodeRuntime,
        fs: Arc<dyn Fs>,
        language_registry: Arc<LanguageRegistry>,
        environment: Entity<ProjectEnvironment>,
        toolchain_store: Arc<dyn LanguageToolchainStore>,
        breakpoint_store: Entity<BreakpointStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.on_app_quit(Self::shutdown_sessions).detach();

        Self {
            mode: DapStoreMode::Local(LocalDapStore {
                fs,
                environment,
                http_client,
                node_runtime,
                toolchain_store,
                language_registry,
                next_session_id: Default::default(),
            }),
            downstream_client: None,
            active_debug_line: None,
            breakpoint_store,
            sessions: Default::default(),
        }
    }

    pub fn new_remote(
        project_id: u64,
        upstream_client: AnyProtoClient,
        breakpoint_store: Entity<BreakpointStore>,
    ) -> Self {
        Self {
            mode: DapStoreMode::Remote(RemoteDapStore {
                upstream_client,
                upstream_project_id: project_id,
                event_queue: Some(VecDeque::default()),
            }),
            downstream_client: None,
            active_debug_line: None,
            breakpoint_store,
            sessions: Default::default(),
        }
    }

    pub fn as_remote(&self) -> Option<&RemoteDapStore> {
        match &self.mode {
            DapStoreMode::Remote(remote_dap_store) => Some(remote_dap_store),
            _ => None,
        }
    }

    pub fn remote_event_queue(&mut self) -> Option<VecDeque<DapStoreEvent>> {
        if let DapStoreMode::Remote(remote) = &mut self.mode {
            remote.event_queue.take()
        } else {
            None
        }
    }

    pub fn as_local(&self) -> Option<&LocalDapStore> {
        match &self.mode {
            DapStoreMode::Local(local_dap_store) => Some(local_dap_store),
            _ => None,
        }
    }

    pub fn as_local_mut(&mut self) -> Option<&mut LocalDapStore> {
        match &mut self.mode {
            DapStoreMode::Local(local_dap_store) => Some(local_dap_store),
            _ => None,
        }
    }

    pub fn upstream_client(&self) -> Option<(AnyProtoClient, u64)> {
        match &self.mode {
            DapStoreMode::Remote(RemoteDapStore {
                upstream_client,
                upstream_project_id,
                ..
            }) => Some((upstream_client.clone(), *upstream_project_id)),

            DapStoreMode::Local(_) => None,
        }
    }

    pub fn downstream_client(&self) -> Option<&(AnyProtoClient, u64)> {
        self.downstream_client.as_ref()
    }

    pub fn add_remote_client(
        &mut self,
        session_id: SessionId,
        ignore: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if let DapStoreMode::Remote(remote) = &self.mode {
            self.sessions.insert(
                session_id,
                cx.new(|_| {
                    debugger::session::Session::remote(
                        session_id,
                        remote.upstream_client.clone(),
                        remote.upstream_project_id,
                        ignore.unwrap_or(false),
                    )
                }),
            );
        } else {
            debug_assert!(false);
        }
    }

    pub fn client_by_id(
        &self,
        session_id: impl Borrow<SessionId>,
    ) -> Option<Entity<session::Session>> {
        let session_id = session_id.borrow();
        let client = self.sessions.get(session_id).cloned();

        client
    }
    pub fn clients(&self) -> impl Iterator<Item = &Entity<Session>> {
        self.sessions.values()
    }

    pub fn capabilities_by_id(
        &self,
        session_id: impl Borrow<SessionId>,
        cx: &App,
    ) -> Option<Capabilities> {
        let session_id = session_id.borrow();
        self.sessions
            .get(session_id)
            .map(|client| client.read(cx).capabilities.clone())
    }

    pub fn update_capabilities_for_client(
        &mut self,
        session_id: SessionId,
        capabilities: &Capabilities,
        cx: &mut Context<Self>,
    ) {
        if let Some(client) = self.client_by_id(session_id) {
            client.update(cx, |this, cx| {
                this.capabilities = this.capabilities.merge(capabilities.clone());
            });
        }

        cx.notify();

        if let Some((downstream_client, project_id)) = self.downstream_client.as_ref() {
            downstream_client
                .send(dap::proto_conversions::capabilities_to_proto(
                    &capabilities,
                    *project_id,
                    session_id.to_proto(),
                ))
                .log_err();
        }
    }

    pub fn active_debug_line(&self) -> Option<(SessionId, ProjectPath, u32)> {
        self.active_debug_line.clone()
    }

    pub fn set_active_debug_line(
        &mut self,
        session_id: &SessionId,
        project_path: &ProjectPath,
        row: u32,
        cx: &mut Context<Self>,
    ) {
        self.active_debug_line = Some((*session_id, project_path.clone(), row));
        cx.emit(DapStoreEvent::ActiveDebugLineChanged);
        cx.notify();
    }

    pub fn remove_active_debug_line_for_client(
        &mut self,
        session_id: &SessionId,
        cx: &mut Context<Self>,
    ) {
        if let Some(active_line) = &self.active_debug_line {
            if active_line.0 == *session_id {
                self.active_debug_line.take();
                cx.emit(DapStoreEvent::ActiveDebugLineChanged);
                cx.notify();

                if let Some((client, project_id)) = self.downstream_client.clone() {
                    client
                        .send(::client::proto::RemoveActiveDebugLine { project_id })
                        .log_err();
                }
            }
        }
    }

    pub fn breakpoint_store(&self) -> &Entity<BreakpointStore> {
        &self.breakpoint_store
    }

    async fn handle_ignore_breakpoint_state(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::IgnoreBreakpointState>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        let session_id = SessionId::from_proto(envelope.payload.session_id);

        this.update(&mut cx, |this, cx| {
            if let Some(client) = this.client_by_id(&session_id) {
                client.update(cx, |client, cx| {
                    client.set_ignore_breakpoints(envelope.payload.ignore)
                });
            }
        })?;

        Ok(())
    }

    pub fn set_ignore_breakpoints(
        &mut self,
        session_id: &SessionId,
        ignore: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.client_by_id(session_id) {
            session.update(cx, |session, _| {
                session.set_ignore_breakpoints(ignore);
            });
        }
    }

    pub fn ignore_breakpoints(&self, session_id: &SessionId, cx: &App) -> bool {
        self.client_by_id(session_id)
            .map(|client| client.read(cx).breakpoints_enabled())
            .unwrap_or_default()
    }

    pub fn toggle_ignore_breakpoints(&mut self, session_id: &SessionId, cx: &mut Context<Self>) {
        if let Some(client) = self.client_by_id(session_id) {
            client.update(cx, |client, _| {
                client.set_ignore_breakpoints(!client.breakpoints_enabled());
            });
        }
    }

    // fn reconnect_client(
    //     &mut self,
    //     adapter: Arc<dyn DebugAdapter>,
    //     binary: DebugAdapterBinary,
    //     config: DebugAdapterConfig,
    //     cx: &mut Context<Self>,
    // ) -> Task<Result<()>> {
    //     if !config.supports_attach && matches!(config.request, DebugRequestType::Attach(_)) {
    //         return Task::ready(Err(anyhow!(
    //             "Debug adapter does not support `attach` request"
    //         )));
    //     }

    //     let session_id = self.as_local().unwrap().next_session_id();

    //     cx.spawn(|dap_store, mut cx| async move {
    //         let mut client = DebugAdapterClient::new(session_id, adapter, binary, &cx);

    //         client
    //             .reconnect(
    //                 {
    //                     let dap_store = dap_store.clone();
    //                     move |message, cx| {
    //                         dap_store
    //                             .update(cx, |_, cx| {
    //                                 cx.emit(DapStoreEvent::DebugClientEvent { session_id, message })
    //                             })
    //                             .log_err();
    //                     }
    //                 },
    //                 &mut cx,
    //             )
    //             .await?;

    //         dap_store.update(&mut cx, |store, cx| {
    //             cx.new(|cx| {
    //                 let client_state =
    //                     debugger::client::Client::local(Arc::new(client), capabilities);
    //             });

    //             store.clients.insert(Arc::new(client), session_id);

    //             // don't emit this event ourself in tests, so we can add request,
    //             // response and event handlers for this client
    //             if !cfg!(any(test, feature = "test-support")) {
    //                 cx.emit(DapStoreEvent::DebugClientStarted(session_id));
    //             }

    //             cx.notify();
    //         })
    //     })
    // }

    fn start_client_internal(
        &mut self,
        delegate: DapAdapterDelegate,
        config: DebugAdapterConfig,
        cx: &mut Context<Self>,
    ) -> Task<Result<Arc<DebugAdapterClient>>> {
        let Some(local_store) = self.as_local_mut() else {
            return Task::ready(Err(anyhow!("cannot start client on remote side")));
        };

        let session_id = local_store.next_session_id();

        cx.spawn(|this, mut cx| async move {
            let adapter = build_adapter(&config.kind).await?;

            if !unimplemented!("adapter.supports_attach()")
                && matches!(config.request, DebugRequestType::Attach(_))
            {
                bail!("Debug adapter does not support `attach` request");
            }

            let binary = cx.update(|cx| {
                let name = DebugAdapterName::from(adapter.name().as_ref());

                ProjectSettings::get_global(cx)
                    .dap
                    .get(&name)
                    .and_then(|s| s.binary.as_ref().map(PathBuf::from))
            })?;

            let (adapter, binary) = match adapter
                .get_binary(&delegate, &config, binary, &mut cx)
                .await
            {
                Err(error) => {
                    delegate.update_status(
                        adapter.name(),
                        DapStatus::Failed {
                            error: error.to_string(),
                        },
                    );

                    return Err(error);
                }
                Ok(mut binary) => {
                    delegate.update_status(adapter.name(), DapStatus::None);

                    let shell_env = delegate.shell_env().await;
                    let mut envs = binary.envs.unwrap_or_default();
                    envs.extend(shell_env);
                    binary.envs = Some(envs);

                    (adapter, binary)
                }
            };

            let mut client = DebugAdapterClient::start(session_id, binary, |_, _| {}, cx).await?;

            Ok(Arc::new(client))
        })
    }

    pub fn start_debug_session(
        &mut self,
        config: DebugAdapterConfig,
        worktree: &Entity<Worktree>,
        cx: &mut Context<Self>,
    ) -> Task<Result<Arc<DebugAdapterClient>>> {
        let Some(local_store) = self.as_local() else {
            return Task::ready(Err(anyhow!("cannot start session on remote side")));
        };

        let delegate = DapAdapterDelegate::new(
            local_store.fs.clone(),
            worktree.read(cx).id(),
            local_store.node_runtime.clone(),
            local_store.http_client.clone(),
            local_store.language_registry.clone(),
            local_store.toolchain_store.clone(),
            local_store.environment.update(cx, |env, cx| {
                let worktree = worktree.read(cx);
                env.get_environment(Some(worktree.id()), Some(worktree.abs_path()), cx)
            }),
        );

        let start_client_task = self.start_client_internal(delegate, config.clone(), cx);

        cx.spawn(|this, mut cx| async move {
            let client = match start_client_task.await {
                Ok(client) => client,
                Err(error) => {
                    this.update(&mut cx, |_, cx| {
                        cx.emit(DapStoreEvent::Notification(error.to_string()));
                    })
                    .log_err();

                    return Err(error);
                }
            };

            this.update(&mut cx, |store, cx| {
                let session_id = client.id();

                unimplemented!("store.clients.insert(session_id, client);");

                cx.emit(DapStoreEvent::DebugClientStarted(session_id));
                cx.notify();

                client
            })
        })
    }

    pub fn configuration_done(
        &self,
        session_id: SessionId,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!("Could not find client: {:?}", session_id)));
        };

        if self
            .capabilities_by_id(session_id, cx)
            .map(|caps| caps.supports_configuration_done_request)
            .flatten()
            .unwrap_or_default()
        {
            cx.background_executor().spawn(async move {
                client
                    .request::<dap::requests::ConfigurationDone>(dap::ConfigurationDoneArguments)
                    .await
            })
        } else {
            Task::ready(Ok(()))
        }
    }

    pub fn new_session(
        &mut self,
        config: DebugAdapterConfig,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        // let config = session.read(cx).as_local().unwrap().configuration();
        // let mut adapter_args = client.adapter().request_args(&config);
        // if let Some(args) = config.initialize_args.clone() {
        //     merge_json_value_into(args, &mut adapter_args);
        // }

        // // TODO(debugger): GDB starts the debuggee program on launch instead of configurationDone
        // // causing our sent breakpoints to not be valid. This delay should eventually be taken out
        // let delay = if &client.adapter_id() == "gdb" {
        //     Some(
        //         cx.background_executor()
        //             .timer(std::time::Duration::from_millis(20u64)),
        //     )
        // } else {
        //     None
        // };

        // cx.background_executor().spawn(async move {
        //     if let Some(delay) = delay {
        //         delay.await;
        //     }

        //     client
        //         .request::<Launch>(LaunchRequestArguments { raw: adapter_args })
        //         .await
        // })
        Task::ready(Ok(()))
    }

    pub fn attach(
        &mut self,
        session_id: SessionId,
        process_id: u32,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        unimplemented!();
        // let Some(client) = self
        //     .client_by_id(session_id)
        //     .and_then(|client| Some(client.read(cx).adapter_client()?))
        // else {
        //     return Task::ready(Err(
        //         anyhow!("Could not find debug client: {:?}", session_id,),
        //     ));
        // };

        // // update the process id on the config, so when the `startDebugging` reverse request
        // // comes in we send another `attach` request with the already selected PID
        // // If we don't do this the user has to select the process twice if the adapter sends a `startDebugging` request
        // session.update(cx, |session, cx| {
        //     session.as_local_mut().unwrap().update_configuration(
        //         |config| {
        //             config.request = DebugRequestType::Attach(task::AttachConfig {
        //                 process_id: Some(process_id),
        //             });
        //         },
        //         cx,
        //     );
        // });

        // let config = session.read(cx).as_local().unwrap().configuration();
        // let mut adapter_args = client.adapter().request_args(&config);

        // if let Some(args) = config.initialize_args.clone() {
        //     merge_json_value_into(args, &mut adapter_args);
        // }

        // cx.background_executor().spawn(async move {
        //     client
        //         .request::<Attach>(AttachRequestArguments { raw: adapter_args })
        //         .await
        // })
    }

    pub fn respond_to_run_in_terminal(
        &self,
        session_id: SessionId,
        success: bool,
        seq: u64,
        body: Option<Value>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!(
                "Could not find debug client: {:?}",
                session_id
            )));
        };

        cx.background_executor().spawn(async move {
            client
                .send_message(Message::Response(Response {
                    seq,
                    body,
                    success,
                    request_seq: seq,
                    command: RunInTerminal::COMMAND.to_string(),
                }))
                .await
        })
    }

    pub fn evaluate(
        &self,
        session_id: &SessionId,
        stack_frame_id: u64,
        expression: String,
        context: EvaluateArgumentsContext,
        source: Option<Source>,
        cx: &mut Context<Self>,
    ) -> Task<Result<EvaluateResponse>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!("Could not find client: {:?}", session_id)));
        };

        cx.background_executor().spawn(async move {
            client
                .request::<Evaluate>(EvaluateArguments {
                    expression: expression.clone(),
                    frame_id: Some(stack_frame_id),
                    context: Some(context),
                    format: None,
                    line: None,
                    column: None,
                    source,
                })
                .await
        })
    }

    pub fn completions(
        &self,
        session_id: &SessionId,
        stack_frame_id: u64,
        text: String,
        completion_column: u64,
        cx: &mut Context<Self>,
    ) -> Task<Result<Vec<CompletionItem>>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!("Could not find client: {:?}", session_id)));
        };

        cx.background_executor().spawn(async move {
            Ok(client
                .request::<Completions>(CompletionsArguments {
                    frame_id: Some(stack_frame_id),
                    line: None,
                    text,
                    column: completion_column,
                })
                .await?
                .targets)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_variable_value(
        &self,
        session_id: &SessionId,
        stack_frame_id: u64,
        variables_reference: u64,
        name: String,
        value: String,
        evaluate_name: Option<String>,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!("Could not find client: {:?}", session_id)));
        };

        let supports_set_expression = self
            .capabilities_by_id(session_id, cx)
            .map(|caps| caps.supports_set_expression)
            .flatten()
            .unwrap_or_default();

        cx.background_executor().spawn(async move {
            if let Some(evaluate_name) = supports_set_expression.then(|| evaluate_name).flatten() {
                client
                    .request::<SetExpression>(SetExpressionArguments {
                        expression: evaluate_name,
                        value,
                        frame_id: Some(stack_frame_id),
                        format: None,
                    })
                    .await?;
            } else {
                client
                    .request::<SetVariable>(SetVariableArguments {
                        variables_reference,
                        name,
                        value,
                        format: None,
                    })
                    .await?;
            }

            Ok(())
        })
    }

    // .. get the client and what not
    // let _ = client.modules(); // This can fire a request to a dap adapter or be a cheap getter.
    // client.wait_for_request(request::Modules); // This ensures that the request that we've fired off runs to completions
    // let returned_value = client.modules(); // this is a cheap getter.

    pub fn shutdown_sessions(&mut self, cx: &mut Context<Self>) -> Task<()> {
        let mut tasks = vec![];
        for session_id in self.sessions.keys().cloned().collect::<Vec<_>>() {
            tasks.push(self.shutdown_session(&session_id, cx));
        }

        cx.background_executor().spawn(async move {
            futures::future::join_all(tasks).await;
        })
    }

    pub fn shutdown_session(
        &mut self,
        session_id: &SessionId,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let Some(_) = self.as_local_mut() else {
            if let Some((upstream_client, project_id)) = self.upstream_client() {
                let future = upstream_client.request(proto::ShutdownDebugClient {
                    project_id,
                    session_id: session_id.to_proto(),
                });

                return cx
                    .background_executor()
                    .spawn(async move { future.await.map(|_| ()) });
            }

            return Task::ready(Err(anyhow!("Cannot shutdown session on remote side")));
        };
        let Some(client) = self.sessions.remove(session_id) else {
            return Task::ready(Err(anyhow!("Could not find session: {:?}", session_id)));
        };

        client.update(cx, |this, cx| {
            this.shutdown(cx);
        });

        Task::ready(Ok(()))
    }

    // async fn _handle_dap_command_2<T: DapCommand + PartialEq + Eq + Hash>(
    //     this: Entity<Self>,
    //     envelope: TypedEnvelope<T::ProtoRequest>,
    //     mut cx: AsyncApp,
    // ) -> Result<<T::ProtoRequest as proto::RequestMessage>::Response>
    // where
    //     <T::DapRequest as dap::requests::Request>::Arguments: Send,
    //     <T::DapRequest as dap::requests::Request>::Response: Send,
    // {
    //     let request = T::from_proto(&envelope.payload);
    //     let session_id = T::session_id_from_proto(&envelope.payload);

    //     let _state = this
    //         .update(&mut cx, |this, cx| {
    //             this.client_by_id(session_id)?
    //                 .read(cx)
    //                 ._wait_for_request(request)
    //         })
    //         .ok()
    //         .flatten();
    //     if let Some(_state) = _state {
    //         let _ = _state.await;
    //     }

    //     todo!()
    // }

    // async fn handle_dap_command<T: DapCommand>(
    //     this: Entity<Self>,
    //     envelope: TypedEnvelope<T::ProtoRequest>,
    //     mut cx: AsyncApp,
    // ) -> Result<<T::ProtoRequest as proto::RequestMessage>::Response>
    // where
    //     <T::DapRequest as dap::requests::Request>::Arguments: Send,
    //     <T::DapRequest as dap::requests::Request>::Response: Send,
    // {
    //     let _sender_id = envelope.original_sender_id().unwrap_or_default();
    //     let session_id = T::session_id_from_proto(&envelope.payload);

    //     let request = T::from_proto(&envelope.payload);
    //     let response = this
    //         .update(&mut cx, |this, cx| {
    //             this.request_dap::<T>(&session_id, request, cx)
    //         })?
    //         .await?;

    //     Ok(T::response_to_proto(&session_id, response))
    // }

    async fn handle_update_debug_adapter(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::UpdateDebugAdapter>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |_, cx| {
            cx.emit(DapStoreEvent::UpdateDebugAdapter(envelope.payload));
        })
    }

    async fn handle_update_thread_status(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::UpdateThreadStatus>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |_, cx| {
            cx.emit(DapStoreEvent::UpdateThreadStatus(envelope.payload));
        })
    }

    async fn handle_set_debug_client_capabilities(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::SetDebugClientCapabilities>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |dap_store, cx| {
            dap_store.update_capabilities_for_client(
                SessionId::from_proto(envelope.payload.session_id),
                &dap::proto_conversions::capabilities_from_proto(&envelope.payload),
                cx,
            );
        })
    }

    async fn handle_shutdown_debug_client(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::ShutdownDebugClient>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |dap_store, cx| {
            let session_id = SessionId::from_proto(envelope.payload.session_id);

            dap_store.client_by_id(session_id).map(|state| {
                state.update(cx, |state, cx| {
                    state.shutdown(cx);
                })
            });

            cx.emit(DapStoreEvent::DebugClientShutdown(session_id));
            cx.notify();
        })
    }

    async fn handle_set_active_debug_line(
        this: Entity<Self>,
        envelope: TypedEnvelope<proto::SetActiveDebugLine>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        let project_path = ProjectPath::from_proto(
            envelope
                .payload
                .project_path
                .context("Invalid Breakpoint call")?,
        );

        this.update(&mut cx, |store, cx| {
            store.active_debug_line = Some((
                SessionId::from_proto(envelope.payload.session_id),
                project_path,
                envelope.payload.row,
            ));

            cx.emit(DapStoreEvent::ActiveDebugLineChanged);
            cx.notify();
        })
    }

    async fn handle_remove_active_debug_line(
        this: Entity<Self>,
        _: TypedEnvelope<proto::RemoveActiveDebugLine>,
        mut cx: AsyncApp,
    ) -> Result<()> {
        this.update(&mut cx, |store, cx| {
            store.active_debug_line.take();

            cx.emit(DapStoreEvent::ActiveDebugLineChanged);
            cx.notify();
        })
    }

    pub fn send_breakpoints(
        &self,
        session_id: SessionId,
        absolute_file_path: Arc<Path>,
        mut breakpoints: Vec<SourceBreakpoint>,
        ignore: bool,
        source_changed: bool,
        cx: &App,
    ) -> Task<Result<()>> {
        let Some(client) = self
            .client_by_id(session_id)
            .and_then(|client| client.read(cx).adapter_client())
        else {
            return Task::ready(Err(anyhow!("Could not find client: {:?}", session_id)));
        };

        // Adjust breakpoints as our client declares that indices start at one.
        breakpoints.iter_mut().for_each(|bp| bp.line += 1u64);

        cx.background_executor().spawn(async move {
            client
                .request::<SetBreakpoints>(SetBreakpointsArguments {
                    source: Source {
                        path: Some(String::from(absolute_file_path.to_string_lossy())),
                        name: absolute_file_path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_string()),
                        source_reference: None,
                        presentation_hint: None,
                        origin: None,
                        sources: None,
                        adapter_data: None,
                        checksums: None,
                    },
                    breakpoints: Some(if ignore { Vec::default() } else { breakpoints }),
                    source_modified: Some(source_changed),
                    lines: None,
                })
                .await?;

            Ok(())
        })
    }

    pub fn send_changed_breakpoints(
        &self,
        project_path: &ProjectPath,
        absolute_path: PathBuf,
        buffer_snapshot: Option<BufferSnapshot>,
        source_changed: bool,
        cx: &App,
    ) -> Task<Result<()>> {
        let source_breakpoints = self
            .breakpoint_store
            .read(cx)
            .breakpoints
            .get(project_path)
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|breakpoint| breakpoint.source_for_snapshot(buffer_snapshot.as_ref()))
            .collect::<Vec<_>>();

        let mut tasks = Vec::new();
        for (session_id, client) in self
            .sessions
            .iter()
            .filter(|(_, client)| client.read(cx).adapter_client().is_some())
        {
            let client = client.read(cx);
            let ignore_breakpoints = !client.breakpoints_enabled();

            tasks.push(self.send_breakpoints(
                *session_id,
                Arc::from(absolute_path.clone()),
                source_breakpoints.clone(),
                ignore_breakpoints,
                source_changed,
                cx,
            ));
        }

        if tasks.is_empty() {
            return Task::ready(Ok(()));
        }

        cx.background_executor().spawn(async move {
            futures::future::join_all(tasks).await;
            Ok(())
        })
    }

    pub fn shared(
        &mut self,
        project_id: u64,
        downstream_client: AnyProtoClient,
        _: &mut Context<Self>,
    ) {
        self.downstream_client = Some((downstream_client.clone(), project_id));
    }

    pub fn unshared(&mut self, cx: &mut Context<Self>) {
        self.downstream_client.take();

        cx.notify();
    }
}

#[derive(Clone)]
pub struct DapAdapterDelegate {
    fs: Arc<dyn Fs>,
    worktree_id: WorktreeId,
    node_runtime: NodeRuntime,
    http_client: Arc<dyn HttpClient>,
    language_registry: Arc<LanguageRegistry>,
    toolchain_store: Arc<dyn LanguageToolchainStore>,
    updated_adapters: Arc<Mutex<HashSet<DebugAdapterName>>>,
    load_shell_env_task: Shared<Task<Option<HashMap<String, String>>>>,
}

impl DapAdapterDelegate {
    pub fn new(
        fs: Arc<dyn Fs>,
        worktree_id: WorktreeId,
        node_runtime: NodeRuntime,
        http_client: Arc<dyn HttpClient>,
        language_registry: Arc<LanguageRegistry>,
        toolchain_store: Arc<dyn LanguageToolchainStore>,
        load_shell_env_task: Shared<Task<Option<HashMap<String, String>>>>,
    ) -> Self {
        Self {
            fs,
            worktree_id,
            http_client,
            node_runtime,
            toolchain_store,
            language_registry,
            load_shell_env_task,
            updated_adapters: Default::default(),
        }
    }
}

#[async_trait(?Send)]
impl dap::adapters::DapDelegate for DapAdapterDelegate {
    fn worktree_id(&self) -> WorktreeId {
        self.worktree_id
    }

    fn http_client(&self) -> Arc<dyn HttpClient> {
        self.http_client.clone()
    }

    fn node_runtime(&self) -> NodeRuntime {
        self.node_runtime.clone()
    }

    fn fs(&self) -> Arc<dyn Fs> {
        self.fs.clone()
    }

    fn updated_adapters(&self) -> Arc<Mutex<HashSet<DebugAdapterName>>> {
        self.updated_adapters.clone()
    }

    fn update_status(&self, dap_name: DebugAdapterName, status: dap::adapters::DapStatus) {
        let name = SharedString::from(dap_name.to_string());
        let status = match status {
            DapStatus::None => BinaryStatus::None,
            DapStatus::Downloading => BinaryStatus::Downloading,
            DapStatus::Failed { error } => BinaryStatus::Failed { error },
            DapStatus::CheckingForUpdate => BinaryStatus::CheckingForUpdate,
        };

        self.language_registry
            .update_dap_status(LanguageServerName(name), status);
    }

    fn which(&self, command: &OsStr) -> Option<PathBuf> {
        which::which(command).ok()
    }

    async fn shell_env(&self) -> HashMap<String, String> {
        let task = self.load_shell_env_task.clone();
        task.await.unwrap_or_default()
    }

    fn toolchain_store(&self) -> Arc<dyn LanguageToolchainStore> {
        self.toolchain_store.clone()
    }
}
