use super::Indicator;

// I: 上游数据源
// IND: 具体的指标逻辑
pub struct IndicatorIter<I, IND> {
    source: I,
    indicator: IND,
}

impl<I, IND> IndicatorIter<I, IND> {
    pub fn new(source: I, indicator: IND) -> Self {
        Self { source, indicator }
    }
}

impl<I, IND> Iterator for IndicatorIter<I, IND>
where
    I: Iterator,
    IND: Indicator<Input = I::Item>,
{
    // 迭代器的 Item 就是指标的 Output
    type Item = IND::Output;

    fn next(&mut self) -> Option<Self::Item> {
        // 1. 从上游拉取
        let input = self.source.next()?;

        // 2. 喂给指标计算
        let output = self.indicator.next_value(input);

        // 3. 返回计算结果
        Some(output)
    }
}

pub trait IndicatorExt: Iterator {
    fn apply<IND>(self, indicator: IND) -> IndicatorIter<Self, IND>
    where
        Self: Sized,
        IND: Indicator<Input = Self::Item>,
    {
        IndicatorIter::new(self, indicator)
    }
}

impl<I: Iterator> IndicatorExt for I {}
