<?php
class P { public function boot(): void { echo 1; } public function init(): void { echo 2; } }
final class C extends P {
    public function boot(): void { parent::boot(); }
    public function init(): void { parent::init(); echo 3; }
}
