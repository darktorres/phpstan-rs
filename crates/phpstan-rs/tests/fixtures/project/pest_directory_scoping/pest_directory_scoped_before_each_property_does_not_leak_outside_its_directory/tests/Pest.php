<?php
            uses(RealTestBase::class)->in('Browser');
            uses()
                ->beforeEach(function (): void { $this->root = 'x'; })
                ->in('Unit');