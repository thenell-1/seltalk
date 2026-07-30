// TODO 人工审查点：1.变量解析正确性 2.去重 3.未提供变量保留占位
// NOTE Prompt 模板渲染：{{var}} 替换；变量提取供前端预览
use std::collections::HashMap;

use crate::error::AppResult;

/// 渲染模板，将 {{var}} 替换为 vars 中的值；未提供的变量保留原占位
pub fn render_template(tpl: &str, vars: &HashMap<String, String>) -> AppResult<String> {
    let mut out = tpl.to_string();
    for (k, v) in vars {
        let placeholder = format!("{{{{{}}}}}", k);
        out = out.replace(&placeholder, v);
    }
    Ok(out)
}

/// 提取模板中的 {{var}} 变量名（去重，保持出现顺序）
pub fn extract_variables(tpl: &str) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    let bytes = tpl.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(j) = tpl[i + 2..].find("}}") {
                let name = tpl[i + 2..i + 2 + j].trim().to_string();
                if !name.is_empty() && !vars.contains(&name) {
                    vars.push(name);
                }
                i = i + 2 + j + 2;
                continue;
            }
        }
        i += 1;
    }
    vars
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_replaces_vars() {
        let mut vars = HashMap::new();
        vars.insert("origin".into(), "你好".into());
        vars.insert("n".into(), "3".into());
        let r = render_template("生成{{n}}条回复：{{origin}}", &vars).unwrap();
        assert_eq!(r, "生成3条回复：你好");
    }

    #[test]
    fn test_extract_variables_no_dups() {
        let v = extract_variables("{{origin}} 和 {{n}} 和 {{origin}}");
        assert_eq!(v, vec!["origin", "n"]);
    }

    #[test]
    fn test_render_missing_var_keeps_placeholder() {
        let vars = HashMap::new();
        let r = render_template("{{origin}}", &vars).unwrap();
        assert_eq!(r, "{{origin}}");
    }
}
