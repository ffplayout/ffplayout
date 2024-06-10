use std::error::Error;

use zeromq::{Socket, SocketRecv, SocketSend, ZmqMessage};

pub async fn zmq_send(msg: &str, socket_addr: &str) -> Result<String, Box<dyn Error>> {
    let mut socket = zeromq::ReqSocket::new();
    socket.connect(&format!("tcp://{socket_addr}")).await?;
    socket.send(msg.into()).await?;
    let repl: ZmqMessage = socket.recv().await?;
    let response = String::from_utf8(repl.into_vec()[0].to_vec())?;

    Ok(response)
}
