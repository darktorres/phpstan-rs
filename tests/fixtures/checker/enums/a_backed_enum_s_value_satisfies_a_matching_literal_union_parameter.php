<?php
            enum SortOrder: string {
                case Asc = 'ASC';
                case Desc = 'DESC';
            }
            class OrderByClause {
                /** @param 'ASC'|'DESC' $dir */
                public function __construct(string $column, string $dir) {}
            }
            function render(SortOrder $order): OrderByClause {
                return new OrderByClause('col', $order->value);
            }
        