use amqprs::{
    callbacks::{DefaultChannelCallback, DefaultConnectionCallback},
    channel::{BasicAckArguments, BasicCancelArguments, BasicConsumeArguments, Channel},
    connection::{Connection, OpenConnectionArguments},
};
use prost::Message;
use rdkafka::{
    producer::{FutureProducer, FutureRecord},
    ClientConfig,
};
use std::{
    env::args,
    time::{self, SystemTime, UNIX_EPOCH},
};

pub mod rust_rmq {
    pub mod sensors {
        include!(concat!(env!("OUT_DIR"), "/rust_rmq.sensors.rs"));
    }
}

use rust_rmq::sensors;

struct RabbitConnect {
    host: String,
    port: u16,
    username: String,
    password: String,
}

fn create_producer(bootstrap_servers: &str) -> FutureProducer {
    ClientConfig::new()
        .set("bootstrap.servers", bootstrap_servers)
        .set("queue.buffering.max.ms", "0")
        .set("compression.type", "gzip")
        .set("linger.ms", "0")
        .set("socket.nagle.disable", "true")
        .create()
        .expect("Failed to create producer")
}

async fn connect_rabbit_mq(connection_details: &RabbitConnect) -> Connection {
    let mut res = Connection::open(
        &OpenConnectionArguments::new(
            &connection_details.host,
            connection_details.port,
            &connection_details.username,
            &connection_details.password,
        )
        .virtual_host("customers"),
    )
    .await;

    while res.is_err() {
        println!("trying to connect after an error");
        std::thread::sleep(time::Duration::from_millis(2000));
        res = Connection::open(
            &OpenConnectionArguments::new(
                &connection_details.host,
                connection_details.port,
                &connection_details.username,
                &connection_details.password,
            )
            .virtual_host("customers"),
        )
        .await;
    }

    let connection = res.unwrap();
    connection
        .register_callback(DefaultConnectionCallback)
        .await
        .unwrap();

    connection
}

async fn get_scan(connection_details: RabbitConnect) {
    let producer = create_producer(
        &args()
            .skip(1)
            .next()
            .unwrap_or("localhost:9092".to_string()),
    );
    loop {
        let mut connection = connect_rabbit_mq(&connection_details).await;
        let mut channel = channel_rabbitmq(&connection).await;
        let args = BasicConsumeArguments::new("customers_created", "");
        let (ctag, mut messages_rx) = channel.basic_consume_rx(args.clone()).await.unwrap();

        while let Some(msg) = messages_rx.recv().await {
            let mut key = 0_usize;
            let a = msg.content.unwrap();
            println!("{:?}", a);
            producer
                .send_result(
                    FutureRecord::to("lidarscan_rmq")
                        .key(&key.to_string())
                        .payload(&a)
                        .timestamp(now()),
                )
                .unwrap()
                .await
                .unwrap()
                .unwrap();
            key += 1;
            let ack = BasicAckArguments::new(msg.deliver.unwrap().delivery_tag(), false);
            let _ = channel.basic_ack(ack).await;
        }

        if let Err(err) = channel.basic_cancel(BasicCancelArguments::new(&ctag)).await {
            println!("Error {}", err.to_string());
        }
    }
}

async fn channel_rabbitmq(connection: &Connection) -> Channel {
    let channel = connection.open_channel(None).await.unwrap();
    channel
        .register_callback(DefaultChannelCallback)
        .await
        .unwrap();

    channel
}

#[tokio::main]
async fn main() {
    let connection_details = RabbitConnect {
        host: "localhost".to_string(),
        port: 5672,
        username: "seven-admin".to_string(),
        password: "123456".to_string(),
    };

    let t1 = tokio::spawn(async { get_scan(connection_details).await });
    println!("Okay we are running async with a dedicated consumer task...");
    let _ = t1.await;
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}
