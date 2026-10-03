<?php
class Foo {
    public int $x;
    public function __construct(int $k) {
        switch ($k) {
            case 1:
                $this->x = 1;
            case 2:
                $this->x = 2;
                break;
            default:
                $this->x = 3;
        }
        echo $this->x;
    }
}
