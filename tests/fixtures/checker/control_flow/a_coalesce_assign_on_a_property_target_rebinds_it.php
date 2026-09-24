<?php
            class Foo {}
            class Bar {
                private ?Foo $watermark = null;
                public function getWatermark(): Foo
                {
                    $this->watermark ??= new Foo();
                    return $this->watermark;
                }
            }
        