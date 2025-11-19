use crate::Symbol;
use dashmap::DashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Symbol 注册表（支持大量交易对）
pub type SymbolRegistry = IdRegistry<Symbol, usize>;

/// Strategy 注册表（最多 64 个策略）
pub type StrategyRegistry = IdRegistry<Symbol, u8>;

/// Exchange 注册表（最多 256 个交易所）
pub type ExchangeRegistry = IdRegistry<Symbol, u8>;

/// 通用的 ID 注册表
///
/// # 类型参数
/// - `K`: Key 类型（如 String）
/// - `V`: ID 类型（如 u8, u16, usize）
pub struct IdRegistry<K, V>
where
    K: Hash + Eq + Clone + Debug,
    V: Copy + Debug + TryFrom<usize>,
{
    /// 名称 -> ID
    name_to_id: DashMap<K, V>,
    /// ID -> 名称
    id_to_name: DashMap<V, K>,
    /// 下一个可用的 ID
    next_id: AtomicUsize,
    /// ID 类型的最大值（用于边界检查）
    max_id: usize,
}

impl<K, V> IdRegistry<K, V>
where
    K: Hash + Eq + Clone + Debug,
    V: Hash + Eq + Clone + Debug + Copy + TryFrom<usize> + Into<usize>,
{
    pub fn new() -> Self {
        Self {
            name_to_id: DashMap::new(),
            id_to_name: DashMap::new(),
            next_id: AtomicUsize::new(0),
            max_id: usize::MAX,
        }
    }

    pub fn with_max_id(max_id: usize) -> Self {
        Self {
            name_to_id: DashMap::new(),
            id_to_name: DashMap::new(),
            next_id: AtomicUsize::new(0),
            max_id,
        }
    }

    /// 获取或注册一个新 ID
    ///
    /// # 返回
    /// - `Ok(id)`: 成功返回 ID
    /// - `Err(msg)`: ID 耗尽
    pub fn get_or_register(&self, name: K) -> Result<V, &'static str> {
        // 快速路径：已存在
        if let Some(id) = self.name_to_id.get(&name) {
            return Ok(*id);
        }

        // 慢路径：分配新 ID
        let id_raw = self.next_id.fetch_add(1, Ordering::Relaxed);

        if id_raw > self.max_id {
            return Err("ID exhausted");
        }

        // 尝试转换为目标类型
        let id = V::try_from(id_raw).map_err(|_| "ID conversion failed")?;

        self.name_to_id.insert(name.clone(), id);
        self.id_to_name.insert(id, name);

        Ok(id)
    }

    /// 通过 ID 查找名称
    #[inline]
    pub fn get_name(&self, id: V) -> Option<K> {
        self.id_to_name.get(&id).map(|r| r.clone())
    }

    /// 通过名称查找 ID
    #[inline]
    pub fn get_id(&self, name: &K) -> Option<V> {
        self.name_to_id.get(name).map(|r| *r)
    }

    /// 检查名称是否已注册
    #[inline]
    pub fn contains(&self, name: &K) -> bool {
        self.name_to_id.contains_key(name)
    }

    /// 已注册的数量
    #[inline]
    pub fn len(&self) -> usize {
        self.name_to_id.len()
    }

    /// 是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.name_to_id.is_empty()
    }

    /// 迭代所有注册项
    pub fn iter(&self) -> impl Iterator<Item = (K, V)> + '_ {
        self.name_to_id
            .iter()
            .map(|r| (r.key().clone(), *r.value()))
    }
}

impl<K, V> Default for IdRegistry<K, V>
where
    K: Hash + Eq + Clone + Debug,
    V: Hash + Eq + Clone + Debug + Copy + TryFrom<usize> + Into<usize>,
{
    fn default() -> Self {
        Self::new()
    }
}
