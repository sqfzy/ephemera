use std::future::Future;

pub trait Strategy: Send + Sync {
    /// 输入数据类型
    type Input;
    /// 输出信号类型
    type Signal;
    /// 错误类型
    type Error;

    /// 处理新的市场数据
    fn on_data(
        &mut self,
        input: Self::Input,
    ) -> impl Future<Output = Result<Option<Self::Signal>, Self::Error>> + Send;
}
