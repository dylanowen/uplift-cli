use futures::task::Context;
use futures::Stream;
use std::pin::Pin;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::task::Poll;

pub trait GroupBy: Stream + Sized {
    fn group_by<Out, GroupFn, MapFn>(
        self,
        grouper: GroupFn,
        mapper: MapFn,
    ) -> GroupReceiver<Self, Out, MapFn>
    where
        GroupFn: Fn(&Self::Item) -> bool + Send + 'static,
        MapFn: Fn(Self::Item) -> Out,
    {
        let mut internal = InternalReceiver {
            receiver: Box::pin(self),
            buffers: vec![],
        };

        let receiver = internal.add_group(grouper);

        GroupReceiver {
            mapper,
            receiver,
            internal: Arc::new(Mutex::new(internal)),
        }
    }
}

impl<T: Stream> GroupBy for T {}

#[must_use = "streams do nothing unless polled"]
pub struct GroupReceiver<St, Out, MapFn>
where
    St: Stream,
    MapFn: Fn(St::Item) -> Out,
{
    mapper: MapFn,
    receiver: Receiver<St::Item>,
    internal: Arc<Mutex<InternalReceiver<St>>>,
}

impl<St, Out, MapFn> GroupReceiver<St, Out, MapFn>
where
    St: Stream,
    MapFn: Fn(St::Item) -> Out,
{
    pub fn add_group<Out1, GroupFn, MapFn1>(
        &self,
        grouper: GroupFn,
        mapper: MapFn1,
    ) -> GroupReceiver<St, Out1, MapFn1>
    where
        GroupFn: Fn(&St::Item) -> bool + Send + 'static,
        MapFn1: Fn(St::Item) -> Out1,
    {
        let receiver = self.internal.lock().unwrap().add_group(grouper);

        GroupReceiver {
            mapper,
            receiver,
            internal: self.internal.clone(),
        }
    }

    fn buffer_fetch(&self) -> Option<Out> {
        match self.receiver.try_recv() {
            Ok(out) => Some((self.mapper)(out)),
            Err(TryRecvError::Empty) => None,
            _ => panic!("We should not be able to disconnect this channel"),
        }
    }
}

impl<St, Out, MapFn> Stream for GroupReceiver<St, Out, MapFn>
where
    St: Stream,
    MapFn: Fn(St::Item) -> Out,
{
    type Item = Out;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            // check our buffer for a result
            if let Some(found) = self.buffer_fetch() {
                return Poll::Ready(Some(found));
            }

            // we didn't find something in our buffer, so ask our upstream for a value
            match self.internal.lock().unwrap().pull(cx) {
                Poll::Ready(true) => (), // loop and check again
                Poll::Ready(false) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

struct SenderGroup<E> {
    grouper: Box<dyn Fn(&E) -> bool + Send>,
    sender: Sender<E>,
}

struct InternalReceiver<St>
where
    St: Stream,
{
    receiver: Pin<Box<St>>,
    buffers: Vec<SenderGroup<St::Item>>,
}

impl<St: Stream> InternalReceiver<St> {
    fn pull(&mut self, cx: &mut Context<'_>) -> Poll<bool> {
        match self.receiver.as_mut().poll_next(cx) {
            Poll::Ready(Some(event)) => {
                if let Some(position) = self.buffers.iter().position(|b| (b.grouper)(&event)) {
                    let sender_group = &self.buffers[position];
                    match sender_group.sender.send(event) {
                        Ok(_) => (), // sent
                        Err(_) => {
                            // we couldn't send the value so drop this sender
                            self.buffers.remove(position);
                        }
                    }
                } else {
                    warn!("Dropping unmatched event")
                }

                // we found something so let whoever is asking know to check their buffer again
                Poll::Ready(true)
            }
            Poll::Ready(None) => Poll::Ready(false),
            Poll::Pending => Poll::Pending,
        }
    }

    fn add_group<GroupFn>(&mut self, grouper: GroupFn) -> Receiver<St::Item>
    where
        GroupFn: Fn(&St::Item) -> bool + Send + 'static,
    {
        let (sender, receiver) = channel();

        self.buffers.push(SenderGroup {
            grouper: Box::new(grouper),
            sender,
        });

        receiver
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use core::mem;
    use futures::channel::mpsc::channel;
    use futures::channel::mpsc::Receiver;
    use futures::channel::mpsc::Sender;
    use futures::sink::SinkExt;
    use futures::{FutureExt, Stream, StreamExt};
    use tokio::task;

    #[tokio::test]
    async fn basic_test() {
        let (mut sender, wrapped) = channel::<usize>(10);

        let mut first = wrapped.group_by(|num| *num > 10, |num| num.to_string());
        let mut second = first.add_group(|num| *num < 5, |num| num + 1);
        sender.send(1).await.unwrap();
        sender.send(20).await.unwrap();
        sender.send(30).await.unwrap();
        sender.send(8).await.unwrap();

        assert_eq!(first.next().await.unwrap(), "20".to_string());
        assert_eq!(first.next().await.unwrap(), "30".to_string());
        assert_eq!(second.next().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn dropping_receiver() {
        let (mut sender, wrapped) = channel::<usize>(10);

        let first = wrapped.group_by(|num| *num > 10, |num| num.to_string());
        let mut second = first.add_group(|num| *num < 5, |num| num + 1);
        sender.send(1).await.unwrap();
        sender.send(20).await.unwrap();
        sender.send(1).await.unwrap();
        mem::drop(first);

        assert_eq!(second.next().await.unwrap(), 2);
        assert_eq!(second.next().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn sending_receiver() {
        let (mut sender, wrapped) = channel::<usize>(10);

        let mut first = wrapped.group_by(|num| *num > 10, |num| num.to_string());
        let first_result = task::spawn(async move { first.next().await.unwrap() });
        sender.send(20).await.unwrap();

        assert_eq!(first_result.await.unwrap(), "20".to_string());
    }
}
