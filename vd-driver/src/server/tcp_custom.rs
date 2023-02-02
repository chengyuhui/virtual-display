use std::time::Instant;

use anyhow::Result;
use tokio::{
    io::{AsyncWriteExt, BufWriter},
    net::TcpStream,
};
use tracing::Instrument;

use crate::{get_app, monitor::CodecData};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum PacketType {
    /// `[i64 ts][data]`
    Video = 0,
    Audio = 1,
    /// `[i64 ts]`
    Timestamp = 2,
    /// `[i32 width][i32 height][u32 len][data][u32 len][data]...`
    Configure = 3,
    /// `[i32 x][i32 y][u32 visible]`
    CursorPosition = 4,
    /// `[data]`
    CursorImage = 5,
}

#[derive(Debug)]
struct VdStream {
    inner: BufWriter<TcpStream>,
}

impl VdStream {
    async fn write_packet(&mut self, ty: PacketType, data: &[&[u8]]) -> Result<()> {
        let total_len = data.iter().map(|d| d.len()).sum::<usize>();

        tracing::trace!("Writing {:?} ({} bytes)", ty, total_len);

        self.inner.write_u32(ty as u32).await?;
        self.inner.write_u32(total_len as u32).await?;
        for d in data {
            self.inner.write_all(d).await?;
        }

        Ok(())
    }

    async fn write_timestamp(&mut self, timestamp: u64) -> Result<()> {
        self.write_packet(PacketType::Timestamp, &[&(timestamp as i64).to_be_bytes()])
            .await
    }

    async fn write_video(&mut self, timestamp: u64, data: &[u8]) -> Result<()> {
        self.write_packet(
            PacketType::Video,
            &[&(timestamp as i64).to_be_bytes(), data],
        )
        .await
    }

    async fn write_configure(&mut self, width: u32, height: u32, data: &[&[u8]]) -> Result<()> {
        let mut pkts = vec![
            (width as i32).to_be_bytes().to_vec(),
            (height as i32).to_be_bytes().to_vec(),
        ];

        for d in data {
            pkts.push((d.len() as u32).to_be_bytes().to_vec());
            pkts.push(d.to_vec());
        }

        let pkts_ref: Vec<&[u8]> = pkts.iter().map(|v| v.as_slice()).collect();
        self.write_packet(PacketType::Configure, &pkts_ref).await
    }

    async fn write_cursor_position(&mut self, x: i32, y: i32, visible: bool) -> Result<()> {
        self.write_packet(
            PacketType::CursorPosition,
            &[
                &x.to_be_bytes(),
                &y.to_be_bytes(),
                &(visible as u32).to_be_bytes(),
            ],
        )
        .await
    }

    async fn write_cursor_image(&mut self, crc32: u32, data: &[u8]) -> Result<()> {
        self.write_packet(PacketType::CursorImage, &[&crc32.to_be_bytes(), data])
            .await
    }

    async fn flush(&mut self) -> Result<()> {
        self.inner.flush().await?;
        Ok(())
    }
}

async fn handle(socket: TcpStream) -> Result<()> {
    socket.set_nodelay(true).ok();

    let socket = tokio::io::BufWriter::with_capacity(1024 * 1024 * 8, socket);

    let monitor = if let Some(monitor) = get_app().monitors().get(&0) {
        monitor.clone()
    } else {
        anyhow::bail!("Monitor 0 not found");
    };
    let mut video_data_rx = monitor.encoded_tx.subscribe();
    let mut encoder_data_rx = monitor.codec_data();
    let mut cursor_position_rx = monitor.cursor_position();
    let mut cursor_image_rx = monitor.cursor_image();

    let mut stream = VdStream { inner: socket };

    // == Timing

    let stream_start = Instant::now();

    {
        // Send initial timestamp packet
        let now = stream_start.elapsed().as_millis() as u64;
        stream.write_timestamp(now).await?;
        stream.flush().await?;
    }

    // Last time we sent a timestamp packet
    let mut last_timestamp_written = Instant::now();

    // == Codec configuration

    let encoder_data = loop {
        tracing::info!("Waiting for codec data");

        if encoder_data_rx.changed().await.is_err() {
            anyhow::bail!("Encoder data channel closed");
        }

        let r = encoder_data_rx.borrow();
        if let Some(data) = r.as_ref() {
            break data.clone();
        }
    };

    tracing::info!("Obtained codec data");

    match encoder_data {
        CodecData::H264 { sps, pps } => {
            stream
                .write_configure(monitor.width(), monitor.height(), &[&sps[..], &pps[..]])
                .await?;
        }
    }

    // == Frames

    loop {
        tokio::select! {
            _ = cursor_position_rx.changed() => {
                let cursor_pos = {
                    let cursor_pos_ref = cursor_position_rx.borrow();
                    if let Some(p) = cursor_pos_ref.as_ref() {
                        *p
                    } else {
                        continue;
                    }
                };

                stream.write_cursor_position(cursor_pos.x, cursor_pos.y, cursor_pos.visible).await?;
            }
            _ = cursor_image_rx.changed() => {
                let cursor_image = {
                    let cursor_image_ref = cursor_image_rx.borrow();
                    if let Some(p) = cursor_image_ref.as_ref() {
                        p.clone()
                    } else {
                        continue;
                    }
                };

                stream.write_cursor_image(cursor_image.crc32, &cursor_image.encoded).await?;
            }
            sample = video_data_rx.recv() => {
                let sample = if let Ok(sample) = sample {
                    sample
                } else {
                    break;
                };

                sample.record_end_to_end_latency();

                stream
                    .write_video(
                        sample.timestamp.duration_since(stream_start).as_millis() as u64,
                        &sample.data,
                    )
                    .await?;

                if last_timestamp_written.elapsed() > std::time::Duration::from_secs(10) {
                    // Send a timestamp packet every 10 seconds, to sync the stream
                    let now = stream_start.elapsed().as_millis() as u64;
                    stream.write_timestamp(now).await?;

                    last_timestamp_written = Instant::now();
                }
            }
        }
        stream.flush().await?;
    }

    Ok(())
}

async fn tcp_server() -> Result<()> {
    let listener = tokio::net::TcpListener::bind("0.0.0.0:9867").await?;

    loop {
        let (socket, addr) = listener.accept().await?;

        let span = tracing::info_span!("tcp_custom", %addr);
        {
            let _enter = span.enter();
            tracing::info!("New connection");
        }

        tokio::spawn(
            async move {
                if let Err(e) = handle(socket).await {
                    tracing::error!(?e, "Connection failed");
                }
            }
            .instrument(span),
        );
    }
}

pub fn start() {
    tokio::spawn(async {
        if let Err(e) = tcp_server().await {
            tracing::error!(?e, "TCP server failed",);
        }
    });
}
