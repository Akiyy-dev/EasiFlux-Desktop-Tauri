import { extractListItems, isRecord, readRecordNumber, readRecordString } from './apiList'

export interface TradeFill {
  fillId: string
  symbol: string
  side: string
  price: string
  qty: string
  fee: string
  execTime: number
}

export interface ClosedPnlRecord {
  symbol: string
  side: string
  closedPnl: string
  closedSize: string
  avgEntryPrice: string
  avgExitPrice: string
  closedTime: number
}

export function parseTradeFills(payload: unknown): TradeFill[] {
  return extractListItems(payload)
    .map((item) => {
      if (!isRecord(item)) {
        return null
      }
      const fillId = readRecordString(item, 'fillId', 'fill_id', 'execId', 'exec_id', 'id')
      if (!fillId) {
        return null
      }
      return {
        fillId,
        symbol: readRecordString(item, 'symbol', 's'),
        side: readRecordString(item, 'side'),
        price: readRecordString(item, 'price', 'execPrice', 'exec_price') || '0',
        qty: readRecordString(item, 'qty', 'execQty', 'exec_qty', 'size') || '0',
        fee: readRecordString(item, 'fee', 'execFee', 'exec_fee') || '0',
        execTime: readRecordNumber(item, 'execTime', 'exec_time', 'time', 'timestamp'),
      }
    })
    .filter((item): item is TradeFill => item !== null)
}

export function parseClosedPnlRecords(payload: unknown): ClosedPnlRecord[] {
  return extractListItems(payload)
    .map((item) => {
      if (!isRecord(item)) {
        return null
      }
      const symbol = readRecordString(item, 'symbol', 's')
      if (!symbol) {
        return null
      }
      return {
        symbol,
        side: readRecordString(item, 'side'),
        closedPnl:
          readRecordString(item, 'closedPnl', 'closed_pnl', 'pnl', 'realisedPnl', 'realised_pnl') ||
          '0',
        closedSize:
          readRecordString(item, 'closedSize', 'closed_size', 'qty', 'size') || '0',
        avgEntryPrice:
          readRecordString(item, 'avgEntryPrice', 'avg_entry_price', 'entryPrice', 'entry_price') ||
          '0',
        avgExitPrice:
          readRecordString(item, 'avgExitPrice', 'avg_exit_price', 'exitPrice', 'exit_price') ||
          '0',
        closedTime: readRecordNumber(item, 'closedTime', 'closed_time', 'updatedTime', 'time'),
      }
    })
    .filter((item): item is ClosedPnlRecord => item !== null)
}
