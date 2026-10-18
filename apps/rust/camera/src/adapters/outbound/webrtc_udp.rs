use std::{
    io::ErrorKind,
    net::{SocketAddr, UdpSocket},
};

pub(crate) struct CandidateSocket {
    pub(crate) socket: UdpSocket,
    pub(crate) address: SocketAddr,
}

#[derive(Debug)]
pub(crate) enum ReceiveError<E> {
    Socket,
    Handler(E),
}

pub(crate) fn receive_one_from_each_socket<E>(
    sockets: &[CandidateSocket],
    recv_buf: &mut [u8],
    mut handle_datagram: impl FnMut(SocketAddr, SocketAddr, &[u8]) -> Result<(), E>,
) -> Result<bool, ReceiveError<E>> {
    let mut received = false;
    for candidate in sockets {
        match candidate.socket.recv_from(recv_buf) {
            Ok((length, source)) => {
                handle_datagram(source, candidate.address, &recv_buf[..length])
                    .map_err(ReceiveError::Handler)?;
                received = true;
            }
            Err(error) if error.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return Err(ReceiveError::Socket),
        }
    }
    Ok(received)
}

pub(crate) fn socket_for_destination(
    sockets: &[CandidateSocket],
    destination: SocketAddr,
) -> Option<&UdpSocket> {
    sockets
        .iter()
        .find(|candidate| candidate.address.is_ipv4() == destination.is_ipv4())
        .map(|candidate| &candidate.socket)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receives_from_ipv6_even_while_ipv4_has_a_backlog() {
        let ipv4_receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        let ipv6_receiver = UdpSocket::bind("[::1]:0").unwrap();
        let ipv4_address = ipv4_receiver.local_addr().unwrap();
        let ipv6_address = ipv6_receiver.local_addr().unwrap();

        let ipv4_sender = UdpSocket::bind("127.0.0.1:0").unwrap();
        let ipv6_sender = UdpSocket::bind("[::1]:0").unwrap();
        ipv4_sender.send_to(b"v4-1", ipv4_address).unwrap();
        ipv4_sender.send_to(b"v4-2", ipv4_address).unwrap();
        ipv6_sender.send_to(b"v6-1", ipv6_address).unwrap();

        let sockets = [
            CandidateSocket {
                socket: ipv4_receiver,
                address: ipv4_address,
            },
            CandidateSocket {
                socket: ipv6_receiver,
                address: ipv6_address,
            },
        ];
        let mut recv_buf = [0; 32];
        let mut first_round = Vec::new();
        assert!(
            receive_one_from_each_socket(&sockets, &mut recv_buf, |source, local, bytes| {
                first_round.push((source, local, bytes.to_vec()));
                Ok::<(), ()>(())
            })
            .unwrap()
        );

        assert_eq!(first_round.len(), 2);
        assert!(first_round[0].0.is_ipv4());
        assert!(first_round[1].0.is_ipv6());
        assert!(first_round[0].1.is_ipv4());
        assert!(first_round[1].1.is_ipv6());
        assert_eq!(first_round[0].2, b"v4-1");
        assert_eq!(first_round[1].2, b"v6-1");

        let (remaining_length, _) = sockets[0].socket.recv_from(&mut recv_buf).unwrap();
        assert_eq!(&recv_buf[..remaining_length], b"v4-2");
    }

    #[test]
    fn routes_udp_packets_through_the_socket_matching_the_address_family() {
        let ipv4_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let ipv6_socket = UdpSocket::bind("[::1]:0").unwrap();
        let sockets = [
            CandidateSocket {
                address: ipv4_socket.local_addr().unwrap(),
                socket: ipv4_socket,
            },
            CandidateSocket {
                address: ipv6_socket.local_addr().unwrap(),
                socket: ipv6_socket,
            },
        ];
        let ipv4_destination: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let ipv6_destination: SocketAddr = "[::1]:9".parse().unwrap();
        assert!(socket_for_destination(&sockets, ipv4_destination)
            .unwrap()
            .local_addr()
            .unwrap()
            .is_ipv4());
        assert!(socket_for_destination(&sockets, ipv6_destination)
            .unwrap()
            .local_addr()
            .unwrap()
            .is_ipv6());
    }
}
