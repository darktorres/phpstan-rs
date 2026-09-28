<?php
final class A {
    private function helper(): int { return 1; }
    private static function sh(): int { return 1; }
    private static function cb(int $a, int $b): int { return $a <=> $b; }
    private function unused(): void {}
    /** @param list<int> $l */
    public function run(array $l): int { usort($l, [self::class, 'CB']); return $this->HELPER() + self::SH(); }
}
