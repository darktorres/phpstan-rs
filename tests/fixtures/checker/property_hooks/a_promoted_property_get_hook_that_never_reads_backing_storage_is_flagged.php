<?php
            class BadGetPromoted {
                public function __construct(
                    public string $y {
                        get => "constant";
                        set(string $value) { $this->y = $value; }
                    }
                ) {}
            }
        