<?php
            class FullyVirtualPromoted {
                public function __construct(
                    public string $name {
                        get => "constant";
                        set(string $value) { echo $value; }
                    }
                ) {}
            }
        