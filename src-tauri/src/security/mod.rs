// TODO 人工审查点：1.unsafe 边界 2.内存释放 3.熵值安全 4.迁移兼容性 5.错误传播
// NOTE Windows DPAPI 封装：对敏感字段（如 API Key）透明加解密
// 加密后的值以 "dpapi:" 前缀 + hex 编码存储，便于区分明文（迁移用）与密文
// DPAPI 绑定当前 Windows 用户，非本机同用户无法解密

#![cfg(target_os = "windows")]

use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
};
use windows::Win32::Foundation::{HLOCAL, LocalFree};

use crate::error::{AppError, AppResult};

/// DPAPI 熵值（额外盐值，防止其他应用调用 DPAPI 解密本应用数据）
/// 注意：熵值本身不是密钥，只是额外的应用级标识，但仍不应泄露
const ENTROPY: &[u8] = b"SelTalk_v1_API_Key_Protection";

/// 加密值前缀，用于区分明文与密文（迁移检测）
pub const DPAPI_PREFIX: &str = "dpapi:";

/// 字节数组 → 十六进制字符串（无需 base64 依赖）
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 十六进制字符串 → 字节数组
fn hex_to_bytes(hex: &str) -> AppResult<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return Err(AppError::Config("hex 长度非偶数".into()));
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .map_err(|e| AppError::Config(format!("hex 解码失败: {e}")))
        })
        .collect()
}

/// 加密明文 → "dpapi:" + hex(encrypted_bytes)
///
/// 安全策略：
/// - CRYPTPROTECT_UI_FORBIDDEN：禁止弹窗（服务端场景必须）
/// - ENTROPY 熵值：应用级标识，其他应用无法解密
/// - 绑定当前 Windows 用户：非本机同用户无法解密
pub fn encrypt(plaintext: &str) -> AppResult<String> {
    let plain_bytes = plaintext.as_bytes();
    let entropy_bytes = ENTROPY;

    unsafe {
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: plain_bytes.len() as u32,
            pbData: plain_bytes.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_bytes.len() as u32,
            pbData: entropy_bytes.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        CryptProtectData(
            &data_in,
            windows::core::PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )
        .map_err(|e| AppError::Config(format!("DPAPI 加密失败: {e}")))?;

        let encrypted =
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec();
        // 释放 DPAPI 分配的内存（必须，否则内存泄漏）
        let _ = LocalFree(HLOCAL(data_out.pbData as *mut _));

        Ok(format!("{DPAPI_PREFIX}{}", bytes_to_hex(&encrypted)))
    }
}

/// 解密 "dpapi:" + hex(encrypted_bytes) → 明文
///
/// 返回值：
/// - Ok(Some(plaintext))：成功解密
/// - Ok(None)：输入不是 dpapi: 前缀（明文，需迁移）
/// - Err(...)：解密失败（数据损坏或非本用户加密）
pub fn decrypt(stored: &str) -> AppResult<Option<String>> {
    // 非加密前缀：返回 None 表示是明文（迁移用）
    if !stored.starts_with(DPAPI_PREFIX) {
        return Ok(None);
    }

    let hex = &stored[DPAPI_PREFIX.len()..];
    let encrypted = hex_to_bytes(hex)?;
    let entropy_bytes = ENTROPY;

    unsafe {
        let data_in = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let entropy = CRYPT_INTEGER_BLOB {
            cbData: entropy_bytes.len() as u32,
            pbData: entropy_bytes.as_ptr() as *mut u8,
        };
        let mut data_out = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        CryptUnprotectData(
            &data_in,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut data_out,
        )
        .map_err(|e| AppError::Config(format!("DPAPI 解密失败: {e}")))?;

        let plain_bytes =
            std::slice::from_raw_parts(data_out.pbData, data_out.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(data_out.pbData as *mut _));

        String::from_utf8(plain_bytes)
            .map(Some)
            .map_err(|e| AppError::Config(format!("解密后 UTF-8 转换失败: {e}")))
    }
}

/// 判断值是否为加密格式（dpapi: 前缀）
pub fn is_encrypted(value: &str) -> bool {
    value.starts_with(DPAPI_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== 正常流程 =====

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        // 加密 → 解密应得到原文
        let plaintext = "sk-abcdef123456789";
        let encrypted = encrypt(plaintext).unwrap();
        assert!(encrypted.starts_with(DPAPI_PREFIX));
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, Some(plaintext.to_string()));
    }

    // ===== 边界场景 =====

    #[test]
    fn test_decrypt_plaintext_returns_none() {
        // 明文（无 dpapi: 前缀）应返回 None（迁移检测用）
        let result = decrypt("sk-plaintext-key").unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_is_encrypted() {
        assert!(!is_encrypted("sk-plaintext"));
        assert!(is_encrypted("dpapi:deadbeef"));
    }

    #[test]
    fn test_bytes_to_hex_roundtrip() {
        let original = vec![0x00, 0xff, 0xab, 0x12];
        let hex = bytes_to_hex(&original);
        assert_eq!(hex, "00ffab12");
        let decoded = hex_to_bytes(&hex).unwrap();
        assert_eq!(decoded, original);
    }

    // ===== 极端场景 =====

    #[test]
    fn test_encrypt_empty_string() {
        let encrypted = encrypt("").unwrap();
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, Some("".to_string()));
    }

    #[test]
    fn test_encrypt_unicode() {
        // Unicode 字符（中文）
        let plaintext = "密钥测试_🔑";
        let encrypted = encrypt(plaintext).unwrap();
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, Some(plaintext.to_string()));
    }

    #[test]
    fn test_hex_to_bytes_odd_length() {
        // 奇数长度 hex 应返回错误
        let result = hex_to_bytes("abc");
        assert!(result.is_err());
    }
}
