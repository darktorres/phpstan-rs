<?php
            abstract class AbsHost {
                abstract public function __construct(private int $a);
            }
            interface IfaceHost {
                public function __construct(public int $b);
            }
        