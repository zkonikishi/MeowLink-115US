//! Native game bridges for explicitly supplied, host-owned endpoint mappings.
//! These are internal Rust types, NOT a second public descriptor/wire contract.
//! Directory traffic is a separate raw numeric-loopback route, never a failed
//! authentication fallback. Public directory TLS awaits a host contract.
use crate::{
    error,
    generic::{self, Identity, Session, Target},
    Result,
};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{copy_bidirectional, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{watch, Mutex},
    task::{JoinHandle, JoinSet},
};
use zeroize::Zeroizing;

pub struct LoggedIn {
    pub identity: Identity,
    target: Target,
    username: String,
    password: Zeroizing<String>,
    initial: Mutex<Option<Session>>,
}
impl LoggedIn {
    pub async fn authenticate(
        target: Target,
        username: String,
        password: Zeroizing<String>,
    ) -> Result<Arc<Self>> {
        let session = generic::login(&target, &username, password.clone()).await?;
        Ok(Arc::new(Self {
            identity: session.identity.clone(),
            target,
            username,
            password,
            initial: Mutex::new(Some(session)),
        }))
    }
    pub async fn refresh(&self) -> Result<()> {
        self.initial.lock().await.take();
        let session = generic::login(&self.target, &self.username, self.password.clone()).await?;
        if session.identity.native_account_id != self.identity.native_account_id
            || session.identity.username != self.username
        {
            return Err(error(
                "simple_identity_changed",
                "账号身份已改变，请重新登录。",
            ));
        }
        *self.initial.lock().await = Some(session);
        Ok(())
    }
    async fn stream(&self, target: &Target) -> Result<Session> {
        let mut expected = self.target.clone();
        expected.port = target.port;
        if &expected != target {
            return Err(error("simple_route", "频道认证身份或目标不匹配。"));
        }
        // A login/preflight stream may have idled while the player selected a
        // channel. Each actual game socket authenticates against its own port.
        self.initial.lock().await.take();
        let session = generic::login(target, &self.username, self.password.clone()).await?;
        if session.identity.native_account_id != self.identity.native_account_id
            || session.identity.username != self.username
        {
            return Err(error(
                "simple_identity_changed",
                "频道返回了不同账号，连接已关闭。",
            ));
        }
        Ok(session)
    }
}
#[derive(Clone)]
pub struct GameRoute {
    pub channel_id: u32,
    pub local_port: u16,
    pub upstream: Target,
}
#[derive(Clone)]
pub struct LoopbackDirectory {
    pub local_port: u16,
    pub upstream: SocketAddr,
}
#[derive(Clone)]
pub struct TlsDirectory {
    pub local_port: u16,
    pub upstream: Target,
}
#[derive(Clone)]
pub enum DirectoryTransport {
    Loopback(LoopbackDirectory),
    Tls(TlsDirectory),
}
/// Transport availability only, not native directory codec/game acceptance.
pub async fn probe_directory_tls(target: &Target) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = target.connect().await?;
        stream
            .shutdown()
            .await
            .map_err(|_| error("simple_directory", "目录 TLS 连接未能正常完成。"))?;
        Ok(())
    })
    .await
    .map_err(|_| error("simple_directory", "目录 TLS 连接超时。"))?
}
#[derive(Clone)]
enum Route {
    Game(Target),
    Directory(SocketAddr),
    DirectoryTls(Target),
}
fn invalid() -> crate::AppError {
    error("simple_route", "频道映射无效或存在冲突，未启动桥接。")
}
fn alias(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 4)), port)
}

fn plan(
    primary: &Target,
    games: &[GameRoute],
    alias_port: u16,
    directory: Option<&LoopbackDirectory>,
) -> Result<Vec<(u16, Route)>> {
    primary.validate()?;
    if games.is_empty() || games.len() > 51 || alias_port == 0 {
        return Err(invalid());
    }
    let mut ids = HashSet::new();
    let mut ports = HashSet::new();
    let mut upstreams = HashSet::new();
    let mut routes = Vec::new();
    for game in games {
        let mut expected = primary.clone();
        expected.port = game.upstream.port;
        if game.local_port == 0
            || !ids.insert(game.channel_id)
            || !ports.insert(game.local_port)
            || !upstreams.insert(game.upstream.port)
            || game.upstream != expected
        {
            return Err(invalid());
        }
        game.upstream.validate()?;
        routes.push((game.local_port, Route::Game(game.upstream.clone())));
    }
    // First means descriptor order, never sort by channel ID or infer base+ID.
    if games[0].upstream != *primary {
        return Err(invalid());
    }
    if let Some(directory) = directory {
        if directory.local_port == 0
            || directory.local_port == alias_port
            || ports.contains(&alias_port)
            || !ports.insert(directory.local_port)
            || !directory.upstream.ip().is_loopback()
            || directory.upstream.port() == 0
            || upstreams.contains(&directory.upstream.port())
        {
            return Err(invalid());
        }
        // TYPE3 startup uses the alias as channel DIRECTORY, not game auth.
        routes.push((alias_port, Route::Directory(directory.upstream)));
        routes.push((directory.local_port, Route::Directory(directory.upstream)));
    } else if ports.contains(&alias_port) {
        if games[0].local_port != alias_port {
            return Err(invalid());
        }
    } else {
        routes.push((alias_port, Route::Game(primary.clone())));
    }
    Ok(routes)
}
async fn bind(routes: Vec<(u16, Route)>) -> Result<Vec<(TcpListener, Route)>> {
    let mut listeners = Vec::new();
    for (port, route) in routes {
        // Nothing is spawned until EVERY bind succeeds. Dropping this vector
        // rolls back our own listeners without disturbing the occupied owner.
        let listener = TcpListener::bind(alias(port)).await.map_err(|_| {
            error(
                "simple_bind",
                "频道端口被占用，桥接未启动；请先处理端口冲突。",
            )
        })?;
        listeners.push((listener, route));
    }
    Ok(listeners)
}
fn plan_transport(
    primary: &Target,
    games: &[GameRoute],
    alias_port: u16,
    directory: Option<&DirectoryTransport>,
) -> Result<Vec<(u16, Route)>> {
    match directory {
        None => plan(primary, games, alias_port, None),
        Some(DirectoryTransport::Loopback(directory)) => {
            plan(primary, games, alias_port, Some(directory))
        }
        Some(DirectoryTransport::Tls(directory)) => {
            let mut expected = primary.clone();
            expected.port = directory.upstream.port;
            directory.upstream.validate()?;
            let mut routes = plan(primary, games, alias_port, None)?;
            if directory.upstream != expected
                || directory.local_port == 0
                || directory.local_port == alias_port
                || games.iter().any(|g| g.local_port == alias_port)
                || routes.iter().any(|(p, _)| *p == directory.local_port)
                || games
                    .iter()
                    .any(|g| g.upstream.port == directory.upstream.port)
            {
                return Err(invalid());
            }
            let alias = routes
                .iter_mut()
                .find(|(port, _)| *port == alias_port)
                .ok_or_else(invalid)?;
            alias.1 = Route::DirectoryTls(directory.upstream.clone());
            routes.push((
                directory.local_port,
                Route::DirectoryTls(directory.upstream.clone()),
            ));
            Ok(routes)
        }
    }
}
pub struct RunningBridges {
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
    bound: Vec<SocketAddr>,
}
impl RunningBridges {
    pub async fn start(
        account: Arc<LoggedIn>,
        games: Vec<GameRoute>,
        alias_port: u16,
        directory: Option<LoopbackDirectory>,
    ) -> Result<Self> {
        Self::start_transport(
            account,
            games,
            alias_port,
            directory.map(DirectoryTransport::Loopback),
        )
        .await
    }
    pub async fn start_transport(
        account: Arc<LoggedIn>,
        games: Vec<GameRoute>,
        alias_port: u16,
        directory: Option<DirectoryTransport>,
    ) -> Result<Self> {
        let listeners = bind(plan_transport(
            &account.target,
            &games,
            alias_port,
            directory.as_ref(),
        )?)
        .await?;
        let bound = listeners
            .iter()
            .map(|(listener, _)| listener.local_addr().map_err(|_| invalid()))
            .collect::<Result<Vec<_>>>()?;
        let (cancel, receiver) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut workers = JoinSet::new();
            for (listener, route) in listeners {
                let account = account.clone();
                let mut receiver = receiver.clone();
                workers.spawn(async move {
                    if *receiver.borrow(){return;}
                    let mut clients=JoinSet::new();
                    loop {
                        tokio::select! {
                            biased;
                            _=receiver.changed()=>break,
                            _=clients.join_next(),if !clients.is_empty()=>{},
                            accepted=listener.accept()=>{
                                let Ok((socket,_))=accepted else {break;};
                                let account=account.clone();let route=route.clone();let mut receiver=receiver.clone();
                                if clients.len()>=128 {drop(socket);continue;}
                                clients.spawn(async move {
                                    if *receiver.borrow(){return;}
                                    tokio::select! {
                                        biased;
                                        _=receiver.changed()=>{},
                                        _=forward(socket,route,account)=>{},
                                    }
                                });
                            }
                        }
                    }
                    clients.abort_all();while clients.join_next().await.is_some(){}
                });
            }
            // Losing one listener invalidates the complete routing set.
            // Do not leave the other channels looking like a healthy setup.
            let _ = workers.join_next().await;
            workers.abort_all();
            while workers.join_next().await.is_some() {}
        });
        Ok(Self {
            cancel,
            task: Some(task),
            bound,
        })
    }
    pub fn is_running(&self) -> bool {
        !*self.cancel.borrow() && self.task.as_ref().is_some_and(|task| !task.is_finished())
    }
    pub fn bound_addresses(&self) -> &[SocketAddr] {
        &self.bound
    }
    pub async fn close(mut self) {
        let _ = self.cancel.send(true);
        if let Some(mut task) = self.task.take() {
            if tokio::time::timeout(Duration::from_secs(5), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = task.await;
            }
        }
    }
}
impl Drop for RunningBridges {
    fn drop(&mut self) {
        let _ = self.cancel.send(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn forward(mut local: TcpStream, route: Route, account: Arc<LoggedIn>) {
    match route {
        Route::Game(target) => {
            let authentication = account.stream(&target);
            // Stop a pending authentication when the native peer disconnects.
            let disconnected = async {
                loop {
                    match local.peek(&mut [0u8; 1]).await {
                        Ok(0) | Err(_) => break,
                        _ => tokio::time::sleep(Duration::from_millis(25)).await,
                    }
                }
            };
            let session = tokio::select! {session=authentication=>session,_=disconnected=>return};
            let Ok(mut session) = session else {
                return;
            };
            let _ = copy_bidirectional(&mut local, &mut session.stream).await;
        }
        Route::Directory(address) => {
            if !address.ip().is_loopback() {
                return;
            }
            let upstream =
                tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(address)).await;
            let Ok(Ok(mut upstream)) = upstream else {
                return;
            };
            let _ = copy_bidirectional(&mut local, &mut upstream).await;
        }
        Route::DirectoryTls(target) => {
            // TLS only. The native directory has NO simple-auth framing.
            // Failure closes this route; it never retries raw TCP.
            let upstream = tokio::time::timeout(Duration::from_secs(10), target.connect()).await;
            let Ok(Ok(mut upstream)) = upstream else {
                return;
            };
            let _ = copy_bidirectional(&mut local, &mut upstream).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(port: u16) -> Target {
        Target {
            host: "127.0.0.1".into(),
            port,
            server_name: "localhost".into(),
            authority_id: "a".into(),
            realm_id: "r".into(),
            save_profile_id: "s".into(),
            ca_certificate: None,
        }
    }
    #[test]
    fn descriptor_order_and_scope_are_explicit() {
        let primary = target(30000);
        let mut games = vec![
            GameRoute {
                channel_id: 100,
                local_port: 30000,
                upstream: primary.clone(),
            },
            GameRoute {
                channel_id: 2,
                local_port: 30001,
                upstream: target(30123),
            },
        ];
        assert_eq!(plan(&primary, &games, 7002, None).unwrap().len(), 3);
        games.swap(0, 1);
        assert!(plan(&primary, &games, 7002, None).is_err());
        games.swap(0, 1);
        games[1].upstream.realm_id = "other".into();
        assert!(plan(&primary, &games, 7002, None).is_err());
    }
    #[test]
    fn all_51_routes_and_alias_are_planned_without_guessing_ids() {
        let primary = target(30000);
        let games: Vec<_> = (0..51)
            .map(|i| GameRoute {
                channel_id: 1000 - i as u32,
                local_port: 31000 + i,
                upstream: target(30000 + i),
            })
            .collect();
        let directory = LoopbackDirectory {
            local_port: 32000,
            upstream: "127.0.0.1:33000".parse().unwrap(),
        };
        let routes = plan(&primary, &games, 7002, Some(&directory)).unwrap();
        assert_eq!(routes.len(), 53);
        assert!(
            matches!(routes.iter().find(|(port,_)|*port==7002).unwrap().1,Route::Directory(address) if address==directory.upstream)
        );
    }
    #[test]
    fn directory_is_separate_and_never_public_plaintext() {
        let primary = target(30000);
        let games = [GameRoute {
            channel_id: 1,
            local_port: 30000,
            upstream: primary.clone(),
        }];
        for (local_port, upstream) in [
            (7002, "127.0.0.1:30051"),
            (30000, "127.0.0.1:30051"),
            (30051, "203.0.113.1:30051"),
            (30051, "127.0.0.1:30000"),
        ] {
            assert!(plan(
                &primary,
                &games,
                7002,
                Some(&LoopbackDirectory {
                    local_port,
                    upstream: upstream.parse().unwrap()
                })
            )
            .is_err());
        }
    }
    #[test]
    fn tls_directory_keeps_same_host_scope_trust_and_distinct_port() {
        let primary = target(30000);
        let games = [GameRoute {
            channel_id: 1,
            local_port: 30000,
            upstream: primary.clone(),
        }];
        let directory = TlsDirectory {
            local_port: 30051,
            upstream: target(30051),
        };
        let routes = plan_transport(
            &primary,
            &games,
            7002,
            Some(&DirectoryTransport::Tls(directory.clone())),
        )
        .unwrap();
        assert_eq!(routes.len(), 3);
        assert!(
            matches!(&routes.iter().find(|(port,_)|*port==7002).unwrap().1,Route::DirectoryTls(target) if target==&directory.upstream)
        );
        let mut foreign = directory.clone();
        foreign.upstream.host = "elsewhere.invalid".into();
        assert!(plan_transport(
            &primary,
            &games,
            7002,
            Some(&DirectoryTransport::Tls(foreign))
        )
        .is_err());
        let mut wrong = directory.clone();
        wrong.upstream.server_name = "elsewhere.invalid".into();
        assert!(plan_transport(
            &primary,
            &games,
            7002,
            Some(&DirectoryTransport::Tls(wrong))
        )
        .is_err());
        let mut conflict = directory;
        conflict.local_port = 7002;
        assert!(plan_transport(
            &primary,
            &games,
            7002,
            Some(&DirectoryTransport::Tls(conflict))
        )
        .is_err());
    }
    #[tokio::test]
    async fn partial_bind_failure_rolls_back_only_our_listeners() {
        let occupied = TcpListener::bind(alias(0)).await.unwrap();
        let blocked = occupied.local_addr().unwrap().port();
        let first = TcpListener::bind(alias(0)).await.unwrap();
        let free = first.local_addr().unwrap().port();
        drop(first);
        assert!(bind(vec![
            (free, Route::Game(target(30000))),
            (blocked, Route::Game(target(30001)))
        ])
        .await
        .is_err());
        let recovered = TcpListener::bind(alias(free)).await.unwrap();
        assert_eq!(recovered.local_addr().unwrap().port(), free);
        assert!(TcpStream::connect(alias(blocked)).await.is_ok());
    }
    async fn free_port(ip: &str) -> u16 {
        let listener = TcpListener::bind((ip, 0)).await.unwrap();
        listener.local_addr().unwrap().port()
    }
    fn fixture_account(primary: Target) -> Arc<LoggedIn> {
        Arc::new(LoggedIn {
            identity: Identity {
                authority_id: "a".into(),
                realm_id: "r".into(),
                save_profile_id: "s".into(),
                connection_id: "fixture".into(),
                native_account_id: "1".into(),
                username: "Fixture".into(),
            },
            target: primary,
            username: "Fixture".into(),
            password: Zeroizing::new("fixture-only".into()),
            initial: Mutex::new(None),
        })
    }
    #[tokio::test]
    async fn failed_game_auth_never_dials_directory_and_close_releases_ports() {
        use tokio::io::AsyncReadExt;
        let directory = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let directory_addr = directory.local_addr().unwrap();
        let primary = target(free_port("127.0.0.1").await);
        let local_port = free_port("127.0.0.4").await;
        let directory_port = free_port("127.0.0.4").await;
        let alias_port = free_port("127.0.0.4").await;
        let games = vec![GameRoute {
            channel_id: 1,
            local_port,
            upstream: primary.clone(),
        }];
        let bridges = RunningBridges::start(
            fixture_account(primary),
            games,
            alias_port,
            Some(LoopbackDirectory {
                local_port: directory_port,
                upstream: directory_addr,
            }),
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(alias(local_port)).await.unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(12), client.read(&mut [0; 1]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), directory.accept())
                .await
                .is_err()
        );
        bridges.close().await;
        assert!(TcpListener::bind(alias(local_port)).await.is_ok());
        assert!(TcpListener::bind(alias(directory_port)).await.is_ok());
    }
    #[tokio::test]
    async fn directory_is_raw_loopback_only_and_shutdown_closes_active_streams() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let directory = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = directory.local_addr().unwrap();
        let primary = target(free_port("127.0.0.1").await);
        let local_port = free_port("127.0.0.4").await;
        let directory_port = free_port("127.0.0.4").await;
        let alias_port = free_port("127.0.0.4").await;
        let bridges = RunningBridges::start(
            fixture_account(primary.clone()),
            vec![GameRoute {
                channel_id: 1,
                local_port,
                upstream: primary,
            }],
            alias_port,
            Some(LoopbackDirectory {
                local_port: directory_port,
                upstream: addr,
            }),
        )
        .await
        .unwrap();
        let mut client = TcpStream::connect(alias(alias_port)).await.unwrap();
        client.write_all(b"ping").await.unwrap();
        let (mut peer, _) = tokio::time::timeout(Duration::from_secs(2), directory.accept())
            .await
            .unwrap()
            .unwrap();
        let mut data = [0; 4];
        peer.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"ping");
        peer.write_all(b"pong").await.unwrap();
        client.read_exact(&mut data).await.unwrap();
        assert_eq!(&data, b"pong");
        bridges.close().await;
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), client.read(&mut [0; 1]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), peer.read(&mut [0; 1]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
}
