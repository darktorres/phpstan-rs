<?php
namespace App {
    class Base {
        public function f(int $old): void {}
    }
    class Sub extends Base {
        public function f(int $new): void {}
    }
    function call_it(Base $b): void {
        $b->f(old: 1);
    }
}
