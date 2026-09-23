//! `Project::check_mock_hygiene`.

use std::path::PathBuf;

use super::super::*;

fn check(source: &str, banned: &[&str]) -> Vec<String> {
    let mut project = Project::new();
    project
        .load_analyzed_file(PathBuf::from("t.php"), source)
        .unwrap();
    project.finish_loading();
    let banned: Vec<String> = banned.iter().map(|s| s.to_string()).collect();
    let mut out: Vec<String> = project
        .check_mock_hygiene(&banned)
        .into_values()
        .flatten()
        .map(|d| format!("{} {}", d.span.start.line, d.identifier))
        .collect();
    out.sort();
    out
}

#[test]
fn mock_configuration_misuse_is_reported() {
    let source = "<?php
namespace App\\Tests;
use PHPUnit\\Framework\\TestCase;
use PHPUnit\\Framework\\MockObject\\MockObject;
final class MailerTest extends TestCase {
    private MockObject|Mailer $mailer;
    /** @var Clock|MockObject */
    private $clock;
    private Real $real;
    protected function setUp(): void {
        $this->mailer = $this->createMock(Mailer::class);
        $this->clock = $this->createStub(Clock::class);
    }
    public function testSend(): void {
        $this->mailer->expects($this->any())->method('send');
        $this->mailer->method('send')->willReturn(true);
        $this->clock->method('now')->with(1)->willReturn(1);
        $this->mailer->expects($this->atLeast(0))->method('send')->willReturnOnConsecutiveCalls(1)->willReturnCallback(fn () => 2);
        $this->mailer->expects($this->once())->method('send')->with('x');
        $db = $this->createMock(Database::class);
    }
}
";
    assert_eq!(
        check(source, &["App\\Tests\\Database"]),
        vec![
            "15 mock.expectsAny",
            "16 mock.methodWithoutExpects",
            "17 mock.withWithoutExpects",
            "18 mock.atLeastZero",
            "18 mock.conflictingReturns",
            "20 mock.bannedClass",
            "6 mock.unionPropertyType",
            "8 mock.unionPropertyType",
        ]
    );
}

#[test]
fn a_test_class_of_only_mocks_is_reported() {
    let source = "<?php
use PHPUnit\\Framework\\TestCase;
use PHPUnit\\Framework\\MockObject\\MockObject;
final class OnlyMocksTest extends TestCase {
    private A&MockObject $a;
    private B&MockObject $b;
}
";
    assert_eq!(check(source, &[]), vec!["4 mock.onlyMockProperties"]);
}
