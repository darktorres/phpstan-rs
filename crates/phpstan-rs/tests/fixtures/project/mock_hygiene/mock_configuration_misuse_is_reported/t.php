<?php
namespace App\Tests;
use PHPUnit\Framework\TestCase;
use PHPUnit\Framework\MockObject\MockObject;
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
