use super::Hasher;

pub struct Blake2bHasher;

impl Hasher for Blake2bHasher {
    type Output = [u8; 32];

    #[inline]
    fn hash(data: &[u8]) -> Self::Output {
        // blake2b_simd::Params 可配置输出长度，这里固定 32 字节
        let result = blake2b_simd::Params::new().hash_length(32).hash(data);

        let mut output = [0u8; 32];
        output.copy_from_slice(result.as_bytes());
        output
    }

    #[inline]
    fn hash_pair(left: &Self::Output, right: &Self::Output) -> Self::Output {
        // blake2b_simd 支持增量更新，避免栈上 64 字节拼接
        let result = blake2b_simd::Params::new()
            .hash_length(32)
            .to_state()
            .update(left)
            .update(right)
            .finalize();

        let mut output = [0u8; 32];
        output.copy_from_slice(result.as_bytes());
        output
    }
}
