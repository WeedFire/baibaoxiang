/// 按 Windows 习惯切分命令行字符串：支持双引号/单引号包裹、\" 转义。
///
/// 与 `str::split_whitespace` 不同，它能正确处理含空格的路径，例如
/// `C:\Program Files\App\app.exe --dir "D:\my data"`。
pub fn split_args(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut has_cur = false;
    let mut quote: Option<char> = None;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' if quote.is_none() => {
                quote = Some(c);
                has_cur = true;
            }
            '"' | '\'' if quote == Some(c) => {
                quote = None;
            }
            '\\' if matches!(chars.peek(), Some('"') | Some('\'')) => {
                // 保留被转义的引号本身
                cur.push(chars.next().unwrap());
                has_cur = true;
            }
            c if quote.is_none() && c.is_whitespace() => {
                if has_cur {
                    out.push(std::mem::take(&mut cur));
                    has_cur = false;
                }
            }
            c => {
                cur.push(c);
                has_cur = true;
            }
        }
    }

    if has_cur {
        out.push(cur);
    }
    out
}

/// 判断一个路径/命令行是否需要加引号后再拼接。
pub fn needs_quotes(s: &str) -> bool {
    s.is_empty() || s.chars().any(|c| c.is_whitespace() || c == '"')
}

/// 为单个参数加上必要的引号，用于拼接 cmd.exe 命令行。
pub fn quote_arg(s: &str) -> String {
    if needs_quotes(s) {
        format!("\"{}\"", s.replace('"', r#"\""#))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_simple_tokens() {
        assert_eq!(split_args("--flag value"), vec!["--flag", "value"]);
    }

    #[test]
    fn keeps_quoted_paths_intact() {
        assert_eq!(
            split_args(r#""C:\Program Files\App\app.exe" --dir "D:\my data""#),
            vec!["C:\\Program Files\\App\\app.exe", "--dir", "D:\\my data"]
        );
    }

    #[test]
    fn collapses_extra_whitespace() {
        assert_eq!(split_args("  a   b  "), vec!["a", "b"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("   "), Vec::<String>::new());
    }

    #[test]
    fn supports_single_quotes() {
        assert_eq!(split_args("'hello world' x"), vec!["hello world", "x"]);
    }

    #[test]
    fn empty_quoted_arg_is_preserved() {
        assert_eq!(split_args(r#"a "" b"#), vec!["a", "", "b"]);
    }

    #[test]
    fn escapes_quotes_inside_args() {
        // 引号包裹内的 \" 变成字面量引号，不再结束引号段
        assert_eq!(split_args(r#""say \"hi\"""#), vec![r#"say "hi""#]);
        // 未包裹时，\" 只是字面量引号，空格仍然分词
        assert_eq!(split_args(r#"say \"hi\""#), vec!["say", r#""hi""#]);
    }

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg("with space"), "\"with space\"");
        assert_eq!(quote_arg(""), "\"\"");
    }
}
