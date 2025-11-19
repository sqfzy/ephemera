`Peekable<impl Iterator<Item = T>>`执行`peeking_take_while`时，Iterator仍会消耗最后那个元素，不过它会被缓存到Peekable中。
