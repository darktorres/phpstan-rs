<?php
                class Foo {
                    /** @var int|string */
                    private $x;
                    public function set(): void {
                        $this->x = 1;
                        $this->x = "a";
                    }
                }
            