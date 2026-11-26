use super::*;

#[test]
fn tokens_compare_whole() {
    assert!(same(b"abc", b"abc"));
    assert!(!same(b"abc", b"abd"));
    assert!(!same(b"abc", b"ab"));
    assert!(simple_name("pg-mcp_1"));
    assert!(!simple_name("a b"));
    assert!(!simple_name("--url"));
    assert!(host_like("203.0.113.7") && host_like("vps.example.com") && host_like("[2001:db8::1]"));
    assert!(!host_like("-oProxyCommand=x") && !host_like("a b") && !host_like(""));
}
