use ephemera_data::Symbol;
use rust_decimal::Decimal;
use std::collections::HashMap;

/// 持仓信息
#[derive(Debug, Clone, Default)]
pub struct Position {
    pub symbol: Symbol,
    pub size: Decimal,
    pub avg_price: Decimal,
}

impl Position {
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            size: Decimal::ZERO,
            avg_price: Decimal::ZERO,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.size.is_zero()
    }

    pub fn value(&self, current_price: Decimal) -> Decimal {
        self.size * current_price
    }

    pub fn pnl(&self, current_price: Decimal) -> Decimal {
        self.size * (current_price - self.avg_price)
    }

    pub fn pnl_pct(&self, current_price: Decimal) -> Decimal {
        if self.avg_price.is_zero() {
            Decimal::ZERO
        } else {
            (current_price - self.avg_price) / self.avg_price
        }
    }
}

/// 策略上下文，维护策略运行时状态
#[derive(Debug, Clone, Default)]
pub struct StrategyContext {
    /// 持仓
    positions: HashMap<Symbol, Position>,
    /// 可用资金
    pub available_balance: Decimal,
    /// 总资产
    pub total_balance: Decimal,
}

impl StrategyContext {
    pub fn new(initial_balance: Decimal) -> Self {
        Self {
            positions: HashMap::new(),
            available_balance: initial_balance,
            total_balance: initial_balance,
        }
    }

    pub fn get_position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol)
    }

    pub fn get_position_mut(&mut self, symbol: &Symbol) -> Option<&mut Position> {
        self.positions.get_mut(symbol)
    }

    pub fn add_position(&mut self, symbol: Symbol, size: Decimal, price: Decimal) {
        let position = self
            .positions
            .entry(symbol.clone())
            .or_insert_with(|| Position::new(symbol));

        if position.size.is_zero() {
            position.avg_price = price;
            position.size = size;
        } else {
            let total_cost = position.avg_price * position.size + price * size;
            position.size += size;
            position.avg_price = total_cost / position.size;
        }
    }

    pub fn reduce_position(&mut self, symbol: &Symbol, size: Decimal) -> bool {
        if let Some(position) = self.positions.get_mut(symbol)
            && position.size >= size
        {
            position.size -= size;
            return true;
        }
        false
    }

    pub fn all_positions(&self) -> impl Iterator<Item = (&Symbol, &Position)> {
        self.positions.iter()
    }
}
