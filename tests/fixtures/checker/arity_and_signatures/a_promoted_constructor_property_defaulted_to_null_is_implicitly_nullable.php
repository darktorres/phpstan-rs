<?php
            class Widget {
                public function __construct(public int $size = null) {}
            }
            function test(): void { new Widget(null); }
        