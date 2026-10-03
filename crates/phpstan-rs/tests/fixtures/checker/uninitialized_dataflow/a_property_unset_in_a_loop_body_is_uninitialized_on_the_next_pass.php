<?php
class Foo {
    public int $x;
    public function __construct() {
        $this->x = 1;
        for ($i = 0; $i < 2; $i++) {
            echo $this->x;
            unset($this->x);
        }
    }
}
