use crate::core::block::CompleteBlock;
use crate::crypto::hash::Hashable;
use crate::config::{VERSION, NETWORK_ID, SEED_NODES};
use crate::crypto::hash::Hash;
use crate::globals::get_current_time;
use crate::core::thread_pool::ThreadPool;
use super::connection::Connection;
use super::handshake::Handshake;
use super::error::P2pError;
use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use std::io::prelude::{Write, Read};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream, SocketAddr, Shutdown};
use std::sync::mpsc::{Sender, Receiver, channel};

enum Message {
    SendBytes(u64, Vec<u8>), // peer id, bytes
    AddConnection(Arc<Connection>),
    RemoveConnection(u64),
    Exit,
}

pub struct P2pServer {
    peer_id: u64, // unique peer id
    tag: Option<String>, // node tag sent on handshake
    max_peers: usize,
    multi_threaded: bool,
    bind_address: String,
    thread_pool: Mutex<ThreadPool>,
    connections: HashMap<u64, Arc<Connection>>,
    channels: HashMap<u64, Mutex<Sender<Message>>>
}

impl P2pServer {
    pub fn new(peer_id: u64, tag: Option<String>, max_peers: usize, multi_threaded: bool, bind_address: String) -> Self {
        if let Some(tag) = &tag {
            assert!(tag.len() > 0 && tag.len() <= 16);
        }

        let threads = if multi_threaded {
            max_peers + 1 // 1 thread for new incoming connections
        } else {
            2 // 1 thread for new incoming connections + 1 thread for listening connections
        };

        P2pServer {
            peer_id,
            tag,
            max_peers,
            multi_threaded,
            bind_address,
            thread_pool: Mutex::new(ThreadPool::new(threads)),
            connections: HashMap::new(),
            channels: HashMap::new()
        }
    }

    pub fn start(self) {
        let arc = Arc::new(RwLock::new(self));

        // main thread
        let arc_clone = arc.clone();
        arc.read().unwrap().thread_pool.lock().unwrap().execute(move || {
            let arc = arc_clone;
            println!("Connecting to seed nodes..."); // TODO only if peerlist is empty
            // allocate this buffer only one time, because we are using the same thread
            let mut buffer: [u8; 512] = [0; 512]; // maximum 512 bytes for handshake
            for peer in SEED_NODES {
                let addr: SocketAddr = peer.parse().unwrap();
                let zelf = arc.clone();
                if let Err(e) = P2pServer::connect_to_peer(zelf, &mut buffer, addr) {
                    println!("Error while trying to connect to seed node '{}': {}", peer, e);
                }
            }

            println!("Starting p2p server...");
            let listener = TcpListener::bind(arc.read().unwrap().get_bind_address()).unwrap();

            println!("Waiting for connections...");
            for stream in listener.incoming() { // main thread verify all new connections
                println!("New incoming connection");
                match stream {
                    Ok(stream) => {
                        let zelf = arc.clone();
                        if !zelf.read().unwrap().accept_new_connections() { // if we have already reached the limit, we ignore this new connection
                            println!("Max peers reached, rejecting connection");
                            if let Err(e) = stream.shutdown(Shutdown::Both) {
                                println!("Error while closing & ignoring incoming connection: {}", e);
                            }
                            continue;
                        }

                        if let Err(e) = P2pServer::handle_new_connection(zelf, &mut buffer, stream, false) {
                            println!("Error on new connection: {}", e);
                        }
                    }
                    Err(e) => {
                        println!("Error while accepting new connection: {}", e);
                    }
                }
            }
        });

        // listening connections thread
        {
            let mut lock = arc.write().unwrap();
            if !lock.is_multi_threaded() {
                let (sender, receiver) = channel();
                let peer_id = lock.peer_id;
                lock.channels.insert(peer_id, Mutex::new(sender));
                let arc_clone = arc.clone();
                println!("Starting single thread connection listener...");
                lock.thread_pool.lock().unwrap().execute(move || {
                    // TODO extend buffer as we have verified this peer
                    let mut connections: HashMap<u64, Arc<Connection>> = HashMap::new();
                    let mut buf: [u8; 512] = [0; 512]; // allocate this buffer only one time
                    loop {
                        while let Ok(msg) = receiver.try_recv() {
                            match msg {
                                Message::Exit => {
                                    return;
                                },
                                Message::AddConnection(connection) => {
                                    connections.insert(connection.get_peer_id(), connection);
                                }
                                Message::RemoveConnection(peer_id) => {
                                    connections.remove(&peer_id);
                                }
                                Message::SendBytes(peer_id, bytes) => {
                                    if let Some(connection) = connections.get(&peer_id) {
                                        if let Err(e) = connection.send_bytes(&bytes) {
                                            println!("Error on sending bytes: {}", e);
                                            connections.remove(&peer_id);
                                        }
                                    }
                                }
                            }
                        }

                        for connection in connections.values() {
                            P2pServer::listen_connection(&arc_clone, &mut buf, &connection)
                        }
                    }
                });
            }
        }
    }

    pub fn accept_new_connections(&self) -> bool {
        self.get_peer_count() < self.max_peers
    }

    pub fn get_peer_count(&self) -> usize {
        self.connections.len()
    }

    pub fn get_slots_available(&self) -> usize {
        self.max_peers - self.connections.len()
    }

    pub fn is_connected_to(&self, peer_id: &u64) -> bool {
        self.peer_id == *peer_id || self.connections.contains_key(peer_id)
    }

    pub fn is_connected_to_addr(&self, peer_addr: &SocketAddr) -> bool {
        for connection in self.connections.values() {
            if *connection.get_peer_address() == *peer_addr {
                return true
            }
        }
        false
    }

    pub fn is_multi_threaded(&self) -> bool {
        self.multi_threaded
    }

    pub fn get_bind_address(&self) -> &String {
        &self.bind_address
    }

    // Send a block too all connected peers (block propagation)
    pub fn broadcast_block(&self, block: &CompleteBlock) -> Result<(), P2pError> {
        /*for connection in self.get_connections() {
            connection.send_bytes(&block.to_bytes())?;
        }*/ // TODO Refactor

        Ok(())
    }

    pub fn broadcast_bytes(&self, buf: &[u8]) {
        for connection in self.get_connections() {
            self.send_to_peer(connection.get_peer_id(),buf.to_vec());
        }
    }

    // notify the thread that own the target peer through channel
    pub fn send_to_peer(&self, peer_id: u64, bytes: Vec<u8>) -> bool {
        match self.get_channel_for_connection(&peer_id) { // get channel for connection thread, so the thread owner send it
            Some(chan) => {
                if let Err(e) = chan.lock().unwrap().send(Message::SendBytes(peer_id, bytes)) {
                    println!("Error while trying to send message 'SendBytes': {}", e);
                }
                true
            },
            None => {
                println!("No channel found for peer {}", peer_id);
                false
            }
        }
    }

    fn get_channel_for_connection(&self, peer_id: &u64) -> Option<&Mutex<Sender<Message>>> {
        if self.is_multi_threaded() {
            self.channels.get(peer_id)
        } else {
            self.channels.get(&self.peer_id)
        }
    }

    // return a 'Receiver' struct if we are in multi thread mode
    // in single mode, we only have one channel
    fn add_connection(&mut self, connection: Arc<Connection>) -> Option<Receiver<Message>> {
        let peer_id = connection.get_peer_id();
        match self.connections.insert(peer_id, connection) {
            Some(_) => {
                panic!("Peer ID '{}' is already used!", peer_id); // should not happen
            },
            None => {}
        }
        println!("add new connection (total {}): {}", self.connections.len(), self.bind_address);

        if self.is_multi_threaded() {
            let (sender, receiver) = channel();
            self.channels.insert(peer_id, Mutex::new(sender));
            return Some(receiver);
        }

        None
    }

    fn remove_connection(&mut self, peer_id: &u64) -> bool {
        match self.connections.remove(peer_id) {
            Some(connection) => {
                if !connection.is_closed() {
                    if let Err(e) = connection.close() {
                        println!("Error while closing connection: {}", e);
                    }
                }

                if self.is_multi_threaded() {
                    match self.channels.remove(peer_id) {
                        Some(channel) => {
                            if let Err(e) = channel.lock().unwrap().send(Message::Exit) {
                                println!("Error while trying to send exit command: {}", e);
                            }
                        },
                        None => {}
                    }
                } else {
                    if let Err(e) = self.get_channel_for_connection(peer_id).unwrap().lock().unwrap().send(Message::RemoveConnection(*peer_id)) {
                        println!("Error while trying to send remove connection {} command: {}", peer_id, e);
                    }
                }

                println!("{} disconnected", connection);

                true
            },
            None => false,
        }
    }

    fn get_connections(&self) -> Vec<&Arc<Connection>> {
        self.connections.values().collect()
    }

    fn build_handshake(&self) -> Handshake {
        let mut peers = vec![];
        let mut iter = self.connections.values();
        while peers.len() < Handshake::MAX_LEN {
            match iter.next() {
                Some(v) => {
                    if !v.is_out() { // don't send our clients
                        peers.push(format!("{}", v.get_peer_address()));
                    }
                },
                None => break
            };
        }

        // TODO set correct params: block height, top block hash
        Handshake::new(VERSION.to_owned(), self.tag.clone(), NETWORK_ID, self.peer_id, get_current_time(), 0, Hash::zero(), peers)
    }

    // Verify handshake send by a new connection
    // based on data size, network ID, peers address validity
    // block height and block top hash of this peer (to know if we are on the same chain)
    fn verify_handshake(&self, addr: SocketAddr, stream: TcpStream, handshake: Handshake, out: bool) -> Result<(Connection, Vec<SocketAddr>), P2pError> {
        println!("Handshake: {}", handshake);
        if *handshake.get_network_id() != NETWORK_ID {
            return Err(P2pError::InvalidNetworkID);
        }

        if self.is_connected_to(&handshake.get_peer_id()) {
            if let Err(e) = stream.shutdown(Shutdown::Both) {
                println!("Error while rejecting peer: {}", e);
            }
            return Err(P2pError::PeerIdAlreadyUsed(handshake.get_peer_id()));
        }

        // TODO check block height, check if top hash is equal to block height
        let (connection, str_peers) = handshake.create_connection(stream, addr, out);
        let mut peers: Vec<SocketAddr> = vec![];
        for peer in str_peers {
            let peer_addr: SocketAddr = match peer.parse() {
                Ok(addr) => addr,
                Err(e) => {
                    let _ = connection.close(); // peer send us an invalid socket address, invalid handshake
                    return Err(P2pError::InvalidPeerAddress(format!("{}", e)));
                }
            };

            if !self.is_connected_to_addr(&peer_addr) { // prevent reconnecting to a known p2p server
                peers.push(peer_addr);
            }
        }
        peers = peers.into_iter().take(self.get_slots_available()).collect(); // limit to X slots available
        Ok((connection, peers))
    }

    fn connect_to_peer(zelf: Arc<RwLock<P2pServer>>, buffer: &mut [u8], peer_addr: SocketAddr) -> Result<(), P2pError> {
        println!("Trying to connect to {}", peer_addr);
        match TcpStream::connect(&peer_addr) {
            Ok(mut stream) => {
                let handshake: Handshake = zelf.read().unwrap().build_handshake();
                println!("Sending handshake from server");
                if let Err(e) = stream.write(&handshake.to_bytes()) {
                    return Err(P2pError::OnWrite(format!("{}", e)));
                }

                // wait on Handshake reply & manage this new connection
                P2pServer::handle_new_connection(zelf, buffer, stream, true)?;
            },
            Err(e) => {
                println!("Error while connecting to a new peer: {}", e);
            }
        };

        Ok(())
    }

    // this function handle all new connection on main thread
    // A new connection have to send an Handshake
    // if the handshake is valid, we accept it & register it on server
    fn handle_new_connection(zelf: Arc<RwLock<P2pServer>>, buffer: &mut [u8], mut stream: TcpStream, out: bool) -> Result<(), P2pError> {
        match stream.peer_addr() {
            Ok(addr) => {
                println!("New connection: {}", addr);
                match stream.read(buffer) {
                    Ok(n) => {
                        let handshake = Handshake::from_bytes(&buffer[0..n])?;
                        let (connection, peers) = zelf.read().unwrap().verify_handshake(addr, stream, handshake, out)?;

                        // if it's a outgoing connection, don't send the handshake back
                        // because we have already sent it
                        if !out {
                            let handshake = zelf.read().unwrap().build_handshake(); // TODO don't send same peers list
                            connection.send_bytes(&handshake.to_bytes())?; // send handshake back
                        }

                        // if we reach here, handshake is all good, we can start listening this new peer
                        let peer_id = connection.get_peer_id(); // keep in memory the peer_id outside connection (because of moved value)
                        let arc_connection = Arc::new(connection);

                        // handle connection
                        {
                            // set stream no-blocking
                            match arc_connection.set_blocking(false) {
                                Ok(_) => {
                                    let mut lock = zelf.write().unwrap(); 
                                    // multi threading
                                    if let Some(receiver) = lock.add_connection(arc_connection.clone()) {
                                        let zelf_clone = zelf.clone();
                                        // 1 thread = 1 client
                                        lock.thread_pool.lock().unwrap().execute(move || {
                                            println!("Adding connection to multithread mode!");
                                            // TODO extend buffer as we have verified this peer
                                            let mut connection_buf: [u8; 512] = [0; 512]; // allocate this buffer only one time
                                            while !arc_connection.is_closed() {
                                                while let Ok(msg) = receiver.try_recv() {
                                                    match msg {
                                                        Message::Exit => {
                                                            println!("EXIT!!");
                                                            return;
                                                        },
                                                        Message::SendBytes(_, bytes) => {
                                                            println!("SEND BYTES!");
                                                            if let Err(e) = arc_connection.send_bytes(&bytes) {
                                                                println!("Error on trying to send bytes: {}", e);
                                                                return;
                                                            }
                                                        }
                                                        _ => {
                                                            panic!("Not supported!");
                                                        }
                                                    }
                                                }
                                                // if this is considered as disconnected, stop looping on it
                                                P2pServer::listen_connection(&zelf_clone, &mut connection_buf, &arc_connection);
                                            }
                                        });
                                    } else {
                                        if match lock.get_channel_for_connection(&lock.peer_id) {
                                            Some(channel) => {
                                                if let Err(e) = channel.lock().unwrap().send(Message::AddConnection(arc_connection)) {
                                                    println!("Error on adding new connection in single thread mode: {}", e);
                                                    true
                                                } else {
                                                    false
                                                }
                                            },
                                            None => {
                                                panic!("Something is wrong: no channel for single thread??");
                                            }
                                        } {
                                            lock.remove_connection(&peer_id);
                                        }
                                    }
                                },
                                Err(e) => {
                                    println!("Error while trying to set Connection to no-blocking: {}", e);
                                }
                            }
                        }

                        // try to extend our peer list
                        for peer in peers {
                            if let Err(e) = P2pServer::connect_to_peer(zelf.clone(), buffer, peer) {
                                println!("Error while trying to connect to a peer from {}: {}", peer_id, e);
                            }
                        }
                    },
                    Err(e) => println!("Error while reading handshake: {}", e)
                }
            }
            Err(e) => println!("Error while retrieving peer address: {}", e)
        };

        Ok(())
    }

    // Listen to incoming packets from a connection
    fn listen_connection(zelf: &Arc<RwLock<P2pServer>>, buf: &mut [u8], connection: &Arc<Connection>) {
        match connection.read_bytes(buf) {
            Ok(0) => {
                zelf.write().unwrap().remove_connection(&connection.get_peer_id());
            },
            Ok(n) => {
                println!("{}: {}", connection, String::from_utf8_lossy(&buf[0..n]));
                zelf.read().unwrap().broadcast_bytes(&buf[0..n]);
            }
            Err(ref e) if e.kind() == ErrorKind::WouldBlock => { // shouldn't happens if server is multithreaded
                // Don't do anything
            },
            Err(e) => {
                zelf.write().unwrap().remove_connection(&connection.get_peer_id());
                println!("An error has occured while reading bytes from {}: {}", connection, e);
            }
        };
    }
}