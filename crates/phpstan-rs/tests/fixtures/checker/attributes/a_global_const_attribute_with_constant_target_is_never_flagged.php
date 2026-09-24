<?php
            #[\Attribute(\Attribute::TARGET_CONSTANT)]
            class MyAttr {}

            #[MyAttr]
            const X = 1;
        