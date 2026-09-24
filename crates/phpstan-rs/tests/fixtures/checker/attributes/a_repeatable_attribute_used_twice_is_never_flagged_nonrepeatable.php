<?php
            #[\Attribute(\Attribute::TARGET_ALL | \Attribute::IS_REPEATABLE)]
            class Foo {}
            #[Foo]
            #[Foo]
            class Bar {}
        