<?php
            class BadSetPromoted {
                public function __construct(
                    public string $z {
                        get => $this->z;
                        set(string $value) { echo $value; }
                    }
                ) {}
            }
        