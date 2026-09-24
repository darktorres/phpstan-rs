<?php
class P { public function __construct(int $x = 0) {} }
class C extends P {
    public function __construct() {
        $this->ready();
    }
    private function ready(): void {}
}
class E extends P {
    public function __construct() {}
}
