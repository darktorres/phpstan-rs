<?php
final class Failure extends Exception {}
final class Assert {
    /** @throws Failure */
    public static function same(mixed $a, mixed $b): void { if ($a !== $b) { throw new Failure(); } }
}
final class T {
    /** @throws Failure */
    public function check(string $name): void {
        Assert::$name(1, 2);
    }
    /** @throws Failure */
    public function checkObject(object $o, string $name): void {
        $o->$name();
    }
    public function caught(string $name): string {
        $state = 'before';
        try {
            $state = 'call';
            Assert::$name(1, 2);
            $state = 'after';
        } catch (Failure) {
            if ($state === 'call') {
                return 'failed';
            }
        }
        return $state;
    }
}
