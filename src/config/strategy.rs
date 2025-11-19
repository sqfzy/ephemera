use eyre::{Result, eyre};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ============================================================================
// Validate Trait
// ============================================================================

/// 验证 trait，所有参数类型都需要实现
pub trait Validate {
    fn validate(&self) -> Result<()>;
}

// ============================================================================
// Strategy Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyConfig {
    /// 策略名称（唯一标识）
    pub name: String,

    /// 是否启用
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 策略及其参数
    #[serde(flatten)]
    pub strategy: Strategy,

    /// 风险配置（可选）
    #[serde(default)]
    pub risk: Option<RiskConfig>,
}

fn default_enabled() -> bool {
    true
}

impl StrategyConfig {
    /// 验证整个配置
    pub fn validate(&self) -> Result<()> {
        // 验证策略参数
        self.strategy.validate()?;

        // 验证风险配置
        if let Some(risk) = &self.risk {
            risk.validate()?;
        }

        Ok(())
    }

    /// 创建新的策略配置
    pub fn new(name: String, strategy: Strategy) -> Self {
        Self {
            name,
            enabled: true,
            strategy,
            risk: None,
        }
    }

    /// 设置风险配置
    pub fn with_risk(mut self, risk: RiskConfig) -> Self {
        self.risk = Some(risk);
        self
    }

    /// 禁用策略
    pub fn disable(mut self) -> Self {
        self.enabled = false;
        self
    }
}

// ============================================================================
// Strategy Enum (核心部分)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "params")]
#[serde(rename_all = "PascalCase")]
pub enum Strategy {
    MACross(MACrossParams),
    Rsi(RSIParams),
    Macd(MACDParams),
    Bollinger(BollingerParams),
}

impl Strategy {
    /// 验证策略参数
    pub fn validate(&self) -> Result<()> {
        match self {
            Strategy::MACross(p) => p.validate(),
            Strategy::Rsi(p) => p.validate(),
            Strategy::Macd(p) => p.validate(),
            Strategy::Bollinger(p) => p.validate(),
        }
    }

    /// 获取策略类型名称
    pub fn type_name(&self) -> &str {
        match self {
            Strategy::MACross(_) => "MACross",
            Strategy::Rsi(_) => "Rsi",
            Strategy::Macd(_) => "Macd",
            Strategy::Bollinger(_) => "Bollinger",
        }
    }

    /// 获取交易标的
    pub fn symbol(&self) -> Option<&str> {
        match self {
            Strategy::MACross(p) => Some(&p.symbol),
            Strategy::Rsi(p) => Some(&p.symbol),
            Strategy::Macd(p) => Some(&p.symbol),
            Strategy::Bollinger(p) => Some(&p.symbol),
        }
    }
}

// ============================================================================
// Strategy Parameters
// ============================================================================

/// 移动平均线交叉策略参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MACrossParams {
    pub symbol: String,
    pub fast_period: usize,
    pub slow_period: usize,
    pub position_size: Decimal,
}

impl Validate for MACrossParams {
    fn validate(&self) -> Result<()> {
        if self.fast_period == 0 {
            return Err(eyre!("fast_period must be greater than 0"));
        }
        if self.slow_period == 0 {
            return Err(eyre!("slow_period must be greater than 0"));
        }
        if self.fast_period >= self.slow_period {
            return Err(eyre!(
                "fast_period ({}) must be less than slow_period ({})",
                self.fast_period,
                self.slow_period
            ));
        }
        if self.position_size <= Decimal::ZERO {
            return Err(eyre!("position_size must be greater than 0"));
        }
        if self.symbol.is_empty() {
            return Err(eyre!("symbol cannot be empty"));
        }
        Ok(())
    }
}

/// RSI 策略参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RSIParams {
    pub symbol: String,
    pub period: usize,
    pub oversold: Decimal,
    pub overbought: Decimal,
    pub position_size: Decimal,
}

impl Validate for RSIParams {
    fn validate(&self) -> Result<()> {
        if self.period == 0 {
            return Err(eyre!("period must be greater than 0"));
        }
        if self.oversold >= self.overbought {
            return Err(eyre!(
                "oversold ({}) must be less than overbought ({})",
                self.oversold,
                self.overbought
            ));
        }
        if self.oversold < Decimal::ZERO || self.oversold > Decimal::from(100) {
            return Err(eyre!("oversold must be between 0 and 100"));
        }
        if self.overbought < Decimal::ZERO || self.overbought > Decimal::from(100) {
            return Err(eyre!("overbought must be between 0 and 100"));
        }
        if self.position_size <= Decimal::ZERO {
            return Err(eyre!("position_size must be greater than 0"));
        }
        if self.symbol.is_empty() {
            return Err(eyre!("symbol cannot be empty"));
        }
        Ok(())
    }
}

/// MACD 策略参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MACDParams {
    pub symbol: String,
    pub fast_period: usize,
    pub slow_period: usize,
    pub signal_period: usize,
    pub position_size: Decimal,
}

impl Validate for MACDParams {
    fn validate(&self) -> Result<()> {
        if self.fast_period == 0 {
            return Err(eyre!("fast_period must be greater than 0"));
        }
        if self.slow_period == 0 {
            return Err(eyre!("slow_period must be greater than 0"));
        }
        if self.signal_period == 0 {
            return Err(eyre!("signal_period must be greater than 0"));
        }
        if self.fast_period >= self.slow_period {
            return Err(eyre!(
                "fast_period ({}) must be less than slow_period ({})",
                self.fast_period,
                self.slow_period
            ));
        }
        if self.position_size <= Decimal::ZERO {
            return Err(eyre!("position_size must be greater than 0"));
        }
        if self.symbol.is_empty() {
            return Err(eyre!("symbol cannot be empty"));
        }
        Ok(())
    }
}

/// 布林带策略参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BollingerParams {
    pub symbol: String,
    pub period: usize,
    pub std_dev_multiplier: Decimal,
    pub position_size: Decimal,
}

impl Validate for BollingerParams {
    fn validate(&self) -> Result<()> {
        if self.period == 0 {
            return Err(eyre!("period must be greater than 0"));
        }
        if self.std_dev_multiplier <= Decimal::ZERO {
            return Err(eyre!("std_dev_multiplier must be greater than 0"));
        }
        if self.position_size <= Decimal::ZERO {
            return Err(eyre!("position_size must be greater than 0"));
        }
        if self.symbol.is_empty() {
            return Err(eyre!("symbol cannot be empty"));
        }
        Ok(())
    }
}

// ============================================================================
// Risk Config
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskConfig {
    /// 最大持仓大小
    pub max_position_size: Option<Decimal>,

    /// 止损百分比
    pub stop_loss_pct: Option<Decimal>,

    /// 止盈百分比
    pub take_profit_pct: Option<Decimal>,

    /// 最大回撤百分比
    pub max_drawdown_pct: Option<Decimal>,
}

impl Validate for RiskConfig {
    fn validate(&self) -> Result<()> {
        if let Some(max_pos) = self.max_position_size
            && max_pos <= Decimal::ZERO
        {
            return Err(eyre!("max_position_size must be greater than 0"));
        }

        if let Some(stop_loss) = self.stop_loss_pct
            && (stop_loss <= Decimal::ZERO || stop_loss > Decimal::from(100))
        {
            return Err(eyre!("stop_loss_pct must be between 0 and 100"));
        }

        if let Some(take_profit) = self.take_profit_pct
            && take_profit <= Decimal::ZERO
        {
            return Err(eyre!("take_profit_pct must be greater than 0"));
        }

        if let Some(max_drawdown) = self.max_drawdown_pct
            && (max_drawdown <= Decimal::ZERO || max_drawdown > Decimal::from(100))
        {
            return Err(eyre!("max_drawdown_pct must be between 0 and 100"));
        }

        Ok(())
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_size: None,
            stop_loss_pct: Some(Decimal::from(5)), // 默认 5% 止损
            take_profit_pct: Some(Decimal::from(10)), // 默认 10% 止盈
            max_drawdown_pct: Some(Decimal::from(20)), // 默认 20% 最大回撤
        }
    }
}

// ============================================================================
// Builder Pattern (可选，方便创建配置)
// ============================================================================

impl MACrossParams {
    pub fn new(symbol: impl Into<String>, fast_period: usize, slow_period: usize) -> Self {
        Self {
            symbol: symbol.into(),
            fast_period,
            slow_period,
            position_size: Decimal::from(100),
        }
    }

    pub fn with_position_size(mut self, size: Decimal) -> Self {
        self.position_size = size;
        self
    }
}

impl RSIParams {
    pub fn new(symbol: impl Into<String>, period: usize) -> Self {
        Self {
            symbol: symbol.into(),
            period,
            oversold: Decimal::from(30),
            overbought: Decimal::from(70),
            position_size: Decimal::from(100),
        }
    }

    pub fn with_levels(mut self, oversold: Decimal, overbought: Decimal) -> Self {
        self.oversold = oversold;
        self.overbought = overbought;
        self
    }

    pub fn with_position_size(mut self, size: Decimal) -> Self {
        self.position_size = size;
        self
    }
}

impl MACDParams {
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            fast_period: 12,
            slow_period: 26,
            signal_period: 9,
            position_size: Decimal::from(100),
        }
    }

    pub fn with_periods(mut self, fast: usize, slow: usize, signal: usize) -> Self {
        self.fast_period = fast;
        self.slow_period = slow;
        self.signal_period = signal;
        self
    }

    pub fn with_position_size(mut self, size: Decimal) -> Self {
        self.position_size = size;
        self
    }
}

impl BollingerParams {
    pub fn new(symbol: impl Into<String>, period: usize) -> Self {
        Self {
            symbol: symbol.into(),
            period,
            std_dev_multiplier: Decimal::from(2),
            position_size: Decimal::from(100),
        }
    }

    pub fn with_std_dev(mut self, multiplier: Decimal) -> Self {
        self.std_dev_multiplier = multiplier;
        self
    }

    pub fn with_position_size(mut self, size: Decimal) -> Self {
        self.position_size = size;
        self
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ma_cross_validation() {
        let params = MACrossParams::new("BTCUSDT", 10, 20).with_position_size(Decimal::from(1000));
        assert!(params.validate().is_ok());

        let invalid = MACrossParams::new("BTCUSDT", 20, 10);
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_rsi_validation() {
        let params =
            RSIParams::new("ETHUSDT", 14).with_levels(Decimal::from(30), Decimal::from(70));
        assert!(params.validate().is_ok());

        let invalid =
            RSIParams::new("ETHUSDT", 14).with_levels(Decimal::from(70), Decimal::from(30));
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_strategy_config_serialization() {
        let config = StrategyConfig::new(
            "my_ma_cross".to_string(),
            Strategy::MACross(MACrossParams::new("BTCUSDT", 10, 20)),
        )
        .with_risk(RiskConfig::default());

        let json = serde_json::to_string_pretty(&config).unwrap();
        println!("{}", json);

        let deserialized: StrategyConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "my_ma_cross");
    }

    #[test]
    fn test_tagged_enum_serialization() {
        let strategy = Strategy::MACross(MACrossParams::new("BTCUSDT", 10, 20));
        let json = serde_json::to_string_pretty(&strategy).unwrap();
        println!("{}", json);

        assert!(json.contains(r#""type": "MACross""#));
    }

    #[test]
    fn test_risk_config_validation() {
        let valid_risk = RiskConfig {
            max_position_size: Some(Decimal::from(10000)),
            stop_loss_pct: Some(Decimal::from(5)),
            take_profit_pct: Some(Decimal::from(10)),
            max_drawdown_pct: Some(Decimal::from(20)),
        };
        assert!(valid_risk.validate().is_ok());

        let invalid_risk = RiskConfig {
            max_position_size: Some(Decimal::from(-100)),
            ..Default::default()
        };
        assert!(invalid_risk.validate().is_err());
    }
}

