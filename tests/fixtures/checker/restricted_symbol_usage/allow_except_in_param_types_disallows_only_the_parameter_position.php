<?php
            class LegacyValue {}
            class Bar {
                public function process(LegacyValue $v): void {}
                public function get(): LegacyValue {}
                public function create(): void {
                    $v = new LegacyValue();
                }
            }
        