<?php
use PHPUnit\Framework\TestCase;
use PHPUnit\Framework\MockObject\MockObject;
final class OnlyMocksTest extends TestCase {
    private A&MockObject $a;
    private B&MockObject $b;
}
