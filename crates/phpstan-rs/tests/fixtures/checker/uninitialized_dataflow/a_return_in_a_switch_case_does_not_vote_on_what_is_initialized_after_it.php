<?php
class Foo {
    public int $x;
    public function __construct(int $k) {
        switch ($k) {
            case 1:
                return;
            default:
                $this->x = 2;
        }
        echo $this->x;
    }
}
