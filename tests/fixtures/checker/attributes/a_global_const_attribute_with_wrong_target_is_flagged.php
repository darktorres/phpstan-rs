<?php
            #[\Attribute(\Attribute::TARGET_CLASS)]
            class OnlyClass {}

            #[OnlyClass]
            const X = 1;
        