/// 策略核心trait，所有策略必须实现此接口
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

    /// 获取策略名称
    fn name(&self) -> &str;

    /// 重置策略状态
    fn reset(&mut self);
}

/// 技术指标trait
pub trait Indicator: Send + Sync {
    /// 输入数据类型
    type Input;
    /// 输出值类型
    type Output;

    /// 更新指标值
    fn update(&mut self, input: Self::Input) -> Option<Self::Output>;

    /// 获取当前指标值
    fn value(&self) -> Option<Self::Output>;

    /// 重置指标
    fn reset(&mut self);
}
