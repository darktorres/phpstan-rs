<?php
trait Outer {
    public function f(): void {
        echo $undefinedVar;
        if (!trait_exists('Inner')) {
            trait Inner {
                public function g(): void {}
            }
        }
    }
}
class FirstHost { use Outer; }
class SecondHost { use Outer; }
class InnerHost { use Inner; }
