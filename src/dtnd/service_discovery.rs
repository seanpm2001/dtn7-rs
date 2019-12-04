use crate::core::{DtnPeer, PeerType};
use crate::DTNCORE;
use crate::{peers_add, CONFIG};
use anyhow::{anyhow, Result};
use bp7::EndpointID;
use futures::{try_ready, Future, Poll};
use log::{debug, error, info};
use net2::UdpBuilder;
use serde::{Deserialize, Serialize};
use std::io;
use tokio::net::UdpSocket;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnnouncementPkt {
    eid: EndpointID,
    cl: Vec<(String, u16)>,
}
struct Server {
    socket: UdpSocket,
    buf: Vec<u8>,
}

impl Future for Server {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        loop {
            if let Some((size, peer)) = Some(try_ready!(self.socket.poll_recv_from(&mut self.buf)))
            {
                let deserialized: AnnouncementPkt = match serde_cbor::from_slice(&self.buf[..size])
                {
                    Ok(pkt) => pkt,
                    Err(e) => {
                        error!("{}", e);
                        continue;
                    }
                };
                //let amt = try_ready!(self.socket.poll_send_to(&self.buf[..size], &peer));
                //println!("Echoed {}/{} bytes to {}", amt, size, peer);
                debug!("Packet from {} : {:?}", peer, deserialized);
                let dtnpeer = DtnPeer::new(
                    deserialized.eid.clone(),
                    peer.ip(),
                    PeerType::Dynamic,
                    deserialized
                        .cl
                        .iter()
                        .map(|(scheme, port)| (scheme.into(), Some(*port)))
                        .collect(),
                );
                peers_add(dtnpeer);
            }
        }
    }
}

fn announcer(socket: std::net::UdpSocket) {
    let sock = UdpSocket::from_std(socket, &tokio::reactor::Handle::default()).unwrap();
    debug!("running announcer");

    // Compile list of conversion layers as string vector
    let mut cls: Vec<(String, u16)> = Vec::new();

    for cl in &(*DTNCORE.lock()).cl_list {
        cls.push((cl.to_string(), cl.port()));
    }
    //let nodeid = format!("dtn://{}", (*DTNCORE.lock()).nodeid);
    //let addr = "127.0.0.1:3003".parse().unwrap();
    let addr = "224.0.0.26:3003".parse().unwrap();
    let pkt = AnnouncementPkt {
        eid: format!("dtn://{}", (*CONFIG.lock()).nodeid.clone()).into(),
        cl: cls,
    };
    let anc = sock
        .send_dgram(serde_cbor::to_vec(&pkt).unwrap(), &addr)
        .and_then(|_| Ok(()))
        .map_err(|e| error!("{:?}", e));
    tokio::spawn(anc);
}
pub fn spawn_service_discovery() -> Result<()> {
    let addr: std::net::SocketAddr = "0.0.0.0:3003".parse()?;
    let socket = UdpBuilder::new_v4()?;
    socket.reuse_address(true)?;
    let socket = socket.bind(addr)?;
    // DEBUG: setup multicast on loopback to true
    socket
        .set_multicast_loop_v4(false)
        .expect("error activating multicast loop v4");
    socket
        .join_multicast_v4(&"224.0.0.26".parse()?, &std::net::Ipv4Addr::new(0, 0, 0, 0))
        .expect("error joining multicast v4 group");
    let socket_clone = socket.try_clone()?;
    let sock = UdpSocket::from_std(socket, &tokio::reactor::Handle::default())?;

    //let sock = UdpSocket::bind(&([0, 0, 0, 0], 3003).into()).unwrap();

    info!("Listening on {}", sock.local_addr()?);
    let server = Server {
        socket: sock,
        buf: vec![0; 1024],
    };
    tokio::spawn(server.map_err(|e| println!("server error = {:?}", e)));

    crate::dtnd::cron::spawn_timer((*crate::CONFIG.lock()).announcement_interval, move || {
        announcer(socket_clone.try_clone().expect("couldn't clone the socket"));
    });
    Ok(())
}
