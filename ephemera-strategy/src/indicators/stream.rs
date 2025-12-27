use super::Indicator;
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

// S: 上游数据源 (Stream)
// IND: 具体的指标逻辑
pub struct IndicatorStream<S, IND> {
    source: S,
    indicator: IND,
}

impl<S, IND> IndicatorStream<S, IND> {
    pub fn new(source: S, indicator: IND) -> Self {
        Self { source, indicator }
    }
}

impl<S, IND> Stream for IndicatorStream<S, IND>
where
    S: Stream + Unpin,
    IND: Indicator<Input = S::Item> + Unpin,
{
    // Stream 的 Item 就是指标的 Output
    type Item = IND::Output;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // 1. 从上游拉取
        let input = match ready!(Pin::new(&mut self.source).poll_next(cx)) {
            Some(val) => val,
            None => return Poll::Ready(None),
        };

        // 2. 喂给指标计算
        let output = self.indicator.next_value(input);

        // 3. 返回计算结果
        Poll::Ready(Some(output))
    }
}

pub trait IndicatorStreamExt: Stream {
    fn apply<IND>(self, indicator: IND) -> IndicatorStream<Self, IND>
    where
        Self: Sized + Unpin,
        IND: Indicator<Input = Self::Item> + Unpin,
    {
        IndicatorStream::new(self, indicator)
    }
}

impl<S: Stream> IndicatorStreamExt for S {}
