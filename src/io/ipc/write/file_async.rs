//! Async writer for IPC files.

use std::sync::Arc;
use std::task::Poll;
use arrow_format::ipc::planus::Builder;
use arrow_format::ipc::{Block, Footer, MetadataVersion};
use futures::future::BoxFuture;
use futures::{AsyncWrite, Sink, AsyncWriteExt, FutureExt};
use super::super::IpcField;
pub use super::common::WriteOptions;
use super::common::{encode_chunk, DictionaryTracker, EncodedData};
use super::common_async::{write_continuation, write_message};
use super::schema::serialize_schema;
use super::{default_ipc_fields, schema_to_bytes};
use crate::array::Array;
use crate::chunk::Chunk;
use crate::datatypes::*;
use crate::error::{ArrowError, Result};
use crate::io::ipc::ARROW_MAGIC;

/// Async Arrow file writer
pub struct FileSink<'a, W: AsyncWrite + Unpin + Send + 'a> {
    writer: Option<W>,
    task: Option<BoxFuture<'a, Result<(usize, Option<Block>, Vec<Block>, Option<W>)>>>,
    /// IPC write options
    options: WriteOptions,
    /// Keeps track of dictionaries that have been written
    dictionary_tracker: DictionaryTracker,
    offset: usize,
    fields: Vec<IpcField>,
    record_blocks: Vec<Block>,
    dictionary_blocks: Vec<Block>,
    schema: Schema,
}

impl<'a, W> FileSink<'a, W> where W: AsyncWrite + Unpin + Send + 'a {
    /// Create a new file writer.
    pub fn new(writer: W, schema: &Schema, ipc_fields: Option<Vec<IpcField>>, options: WriteOptions) -> Self {
        let fields = ipc_fields.unwrap_or_else(|| default_ipc_fields(&schema.fields));
        let encoded = EncodedData {
            ipc_message: schema_to_bytes(schema, &fields),
            arrow_data: vec![],
        };
        let task = Some(Self::start(writer, encoded).boxed());
        Self {
            writer: None,
            task,
            options,
            fields,
            offset: 0,
            schema: schema.clone(),
            dictionary_tracker: DictionaryTracker::new(true),
            record_blocks: vec![],
            dictionary_blocks: vec![],
        }
    }

    async fn start(mut writer: W, encoded: EncodedData) -> Result<(usize, Option<Block>, Vec<Block>, Option<W>)> {
        writer.write_all(&ARROW_MAGIC[..]).await?;
        writer.write_all(&[0, 0]).await?;
        let (meta, data) = write_message(&mut writer, encoded).await?;

        Ok((meta + data + 8, None, vec![], Some(writer)))
    }

    async fn write(mut writer: W, mut offset: usize, record: EncodedData, dictionaries: Vec<EncodedData>) -> Result<(usize, Option<Block>, Vec<Block>, Option<W>)> {
        let mut dict_blocks = vec![];
        for dict in dictionaries {
            let (meta, data) = write_message(&mut writer, dict).await?;
            let block = Block {
                offset: offset as i64,
                meta_data_length: meta as i32,
                body_length: data as i64,
            };
            dict_blocks.push(block);
            offset += meta + data;
        }
        let (meta, data) = write_message(&mut writer, record).await?;
        let block = Block {
            offset: offset as i64,
            meta_data_length: meta as i32,
            body_length: data as i64,
        };
        offset += meta + data;
        Ok((offset, Some(block), dict_blocks, Some(writer)))
    }

    async fn finish(mut writer: W, footer: Footer) -> Result<(usize, Option<Block>, Vec<Block>, Option<W>)> {
        write_continuation(&mut writer, 0).await?;
        let footer = {
            let mut builder = Builder::new();
            builder.finish(&footer, None).to_owned()
        };
        writer.write_all(&footer[..]).await?;
        writer.write_all(&(footer.len() as i32).to_le_bytes()).await?;
        writer.write_all(&ARROW_MAGIC).await?;
        writer.close().await?;

        Ok((0, None, vec![], None))
    }

    fn poll_write(&mut self, cx: &mut std::task::Context<'_>) -> Poll<Result<()>> {
        if let Some(task) = &mut self.task {
            match futures::ready!(task.poll_unpin(cx)) {
                Ok((offset, record, mut dictionaries, writer)) => {
                    self.task = None;
                    self.writer = writer;
                    self.offset = offset;
                    if let Some(block) = record {
                        self.record_blocks.push(block);
                    }
                    self.dictionary_blocks.append(&mut dictionaries);
                    Poll::Ready(Ok(()))
                },
                Err(error) => {
                    self.task = None;
                    Poll::Ready(Err(error))
                },
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl<'a, W> Sink<Chunk<Arc<dyn Array>>> for FileSink<'a, W> where W: AsyncWrite + Unpin + Send + 'a {
    type Error = ArrowError;

    fn poll_ready(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<()>> {
        self.get_mut().poll_write(cx)
    }

    fn start_send(self: std::pin::Pin<&mut Self>, item: Chunk<Arc<dyn Array>>) -> Result<()> {
        let this = self.get_mut();

        if let Some(writer) = this.writer.take() {
            let (dictionaries, record) = encode_chunk(
                &item,
                &this.fields[..],
                &mut this.dictionary_tracker,
                &this.options,
            )?;

            this.task = Some(Self::write(writer, this.offset, record, dictionaries).boxed());
            Ok(())
        } else {
            Err(ArrowError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "writer is closed",
            )))
        }
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<()>> {
        self.get_mut().poll_write(cx)
    }

    fn poll_close(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<()>> {
        let this = self.get_mut();
        match futures::ready!(this.poll_write(cx)) {
            Ok(()) => {
                if let Some(writer) = this.writer.take() {
                    let schema = serialize_schema(&this.schema, &this.fields);
                    let footer = Footer {
                        version: MetadataVersion::V5,
                        schema: Some(Box::new(schema)),
                        dictionaries: Some(std::mem::take(&mut this.dictionary_blocks)),
                        record_batches: Some(std::mem::take(&mut this.record_blocks)),
                        custom_metadata: None,
                    };
                    this.task = Some(Self::finish(writer, footer).boxed());
                    this.poll_write(cx)
                } else {
                    Poll::Ready(Ok(()))
                }
            },
            Err(error) => Poll::Ready(Err(error)),
        }
    }
}
